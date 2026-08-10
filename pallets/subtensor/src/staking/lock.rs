use super::*;
use crate::subnets::leasing::LeaseId;
use crate::weights::WeightInfo;
use codec::{Decode, DecodeWithMemTracking, Encode};
use frame_support::weights::{Weight, WeightMeter};
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
/// This struct is stored in state maps. The additional logic is implemented in
// higher level LockState[class] structs.
#[crate::freeze_struct("eedde2cfd95ddcb1")]
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
    pub fn is_dust(&self) -> bool {
        self.locked_mass < AlphaBalance::from(LOCK_STATE_ZERO_THRESHOLD)
            && self.conviction < U64F64::saturating_from_num(LOCK_STATE_ZERO_THRESHOLD)
    }

    fn normalize_dust(mut self) -> Self {
        if self.is_dust() {
            self.locked_mass = AlphaBalance::ZERO;
            self.conviction = U64F64::saturating_from_num(0);
        }
        self
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
        &self,
        dt: u64,
        unlock_rate: u64,
        maturity_rate: u64,
        perpetual_lock: bool,
    ) -> (AlphaBalance, U64F64) {
        let unlock_decay = Self::exp_decay(dt, unlock_rate);
        let maturity_decay = Self::exp_decay(dt, maturity_rate);
        let mass_fixed = U64F64::saturating_from_num(self.locked_mass);
        let new_locked_mass = if perpetual_lock {
            self.locked_mass
        } else {
            unlock_decay
                .saturating_mul(mass_fixed)
                .saturating_to_num::<u64>()
                .into()
        };

        let conviction_from_existing = maturity_decay.saturating_mul(self.conviction);
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

    pub(crate) fn add(&self, other: &Self) -> Self {
        Self {
            locked_mass: self.locked_mass.saturating_add(other.locked_mass),
            conviction: self.conviction.saturating_add(other.conviction),
            last_update: self.last_update,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
struct LockStatePerpetualGeneral {
    lock: LockState,
}

#[derive(Clone, PartialEq, Eq, Debug)]
struct LockStateDecayinglGeneral {
    lock: LockState,
}

#[derive(Clone, PartialEq, Eq, Debug)]
struct LockStatePerpetualOwner {
    lock: LockState,
}

#[derive(Clone, PartialEq, Eq, Debug)]
struct LockStateDecayinglOwner {
    lock: LockState,
}

impl LockStatePerpetualGeneral {
    pub fn roll_forward(&self, now: u64, unlock_rate: u64, maturity_rate: u64) -> Self {
        Self {
            lock: if now > self.lock.last_update {
                let dt = now.saturating_sub(self.lock.last_update);
                let (locked_mass, conviction) = self.lock.calculate_decayed_mass_and_conviction(
                    dt,
                    unlock_rate,
                    maturity_rate,
                    true,
                );

                LockState {
                    locked_mass,
                    conviction,
                    last_update: now,
                }
            } else {
                self.lock.clone()
            },
        }
    }
}

impl LockStateDecayinglGeneral {
    pub fn roll_forward(&self, now: u64, unlock_rate: u64, maturity_rate: u64) -> Self {
        Self {
            lock: if now > self.lock.last_update {
                let dt = now.saturating_sub(self.lock.last_update);
                let (locked_mass, conviction) = self.lock.calculate_decayed_mass_and_conviction(
                    dt,
                    unlock_rate,
                    maturity_rate,
                    false,
                );

                LockState {
                    locked_mass,
                    conviction,
                    last_update: now,
                }
            } else {
                self.lock.clone()
            },
        }
    }
}

impl LockStatePerpetualOwner {
    pub fn roll_forward(&self, now: u64, unlock_rate: u64, maturity_rate: u64) -> Self {
        let mut lock = if now > self.lock.last_update {
            let dt = now.saturating_sub(self.lock.last_update);
            let (locked_mass, conviction) = self.lock.calculate_decayed_mass_and_conviction(
                dt,
                unlock_rate,
                maturity_rate,
                true,
            );

            LockState {
                locked_mass,
                conviction,
                last_update: now,
            }
        } else {
            self.lock.clone()
        };
        lock.conviction = U64F64::saturating_from_num(u64::from(lock.locked_mass));
        Self { lock }
    }
}

impl LockStateDecayinglOwner {
    pub fn roll_forward(&self, now: u64, unlock_rate: u64, maturity_rate: u64) -> Self {
        let mut lock = if now > self.lock.last_update {
            let dt = now.saturating_sub(self.lock.last_update);
            let (locked_mass, conviction) = self.lock.calculate_decayed_mass_and_conviction(
                dt,
                unlock_rate,
                maturity_rate,
                false,
            );

            LockState {
                locked_mass,
                conviction,
                last_update: now,
            }
        } else {
            self.lock.clone()
        };
        lock.conviction = U64F64::saturating_from_num(u64::from(lock.locked_mass));
        Self { lock }
    }
}

/// Class of lock that is determined by owner and perpetual flags.
#[derive(Clone, PartialEq, Eq, Debug)]
enum LockClass {
    PerpetualGeneral(LockStatePerpetualGeneral),
    DecayingGeneral(LockStateDecayinglGeneral),
    PerpetualOwner(LockStatePerpetualOwner),
    DecayingOwner(LockStateDecayinglOwner),
}

impl LockClass {
    pub(crate) fn new(lock: LockState, owner: bool, perpetual: bool) -> Self {
        match (owner, perpetual) {
            (false, true) => Self::PerpetualGeneral(LockStatePerpetualGeneral { lock }),
            (false, false) => Self::DecayingGeneral(LockStateDecayinglGeneral { lock }),
            (true, true) => Self::PerpetualOwner(LockStatePerpetualOwner { lock }),
            (true, false) => Self::DecayingOwner(LockStateDecayinglOwner { lock }),
        }
    }

    fn lock(&self) -> &LockState {
        match self {
            Self::PerpetualGeneral(state) => &state.lock,
            Self::DecayingGeneral(state) => &state.lock,
            Self::PerpetualOwner(state) => &state.lock,
            Self::DecayingOwner(state) => &state.lock,
        }
    }

    fn lock_mut(&mut self) -> &mut LockState {
        match self {
            Self::PerpetualGeneral(state) => &mut state.lock,
            Self::DecayingGeneral(state) => &mut state.lock,
            Self::PerpetualOwner(state) => &mut state.lock,
            Self::DecayingOwner(state) => &mut state.lock,
        }
    }

    pub(crate) fn into_lock(self) -> LockState {
        match self {
            Self::PerpetualGeneral(state) => state.lock,
            Self::DecayingGeneral(state) => state.lock,
            Self::PerpetualOwner(state) => state.lock,
            Self::DecayingOwner(state) => state.lock,
        }
    }

    pub(crate) fn roll_forward(&self, now: u64, unlock_rate: u64, maturity_rate: u64) -> Self {
        match self {
            Self::PerpetualGeneral(state) => {
                Self::PerpetualGeneral(state.roll_forward(now, unlock_rate, maturity_rate))
            }
            Self::DecayingGeneral(state) => {
                Self::DecayingGeneral(state.roll_forward(now, unlock_rate, maturity_rate))
            }
            Self::PerpetualOwner(state) => {
                Self::PerpetualOwner(state.roll_forward(now, unlock_rate, maturity_rate))
            }
            Self::DecayingOwner(state) => {
                Self::DecayingOwner(state.roll_forward(now, unlock_rate, maturity_rate))
            }
        }
    }

    fn flags(&self) -> (bool, bool) {
        match self {
            Self::PerpetualGeneral(_) => (false, true),
            Self::DecayingGeneral(_) => (false, false),
            Self::PerpetualOwner(_) => (true, true),
            Self::DecayingOwner(_) => (true, false),
        }
    }
}

pub fn roll_lock_state(
    lock: LockState,
    now: u64,
    unlock_rate: u64,
    maturity_rate: u64,
    owner: bool,
    perpetual: bool,
) -> LockState {
    LockClass::new(lock, owner, perpetual)
        .roll_forward(now, unlock_rate, maturity_rate)
        .into_lock()
        .normalize_dust()
}

/// A struct that incapsulates Lock primitives such as adding, removing,
/// rolling, and updating aggregates.
pub struct ConvictionModel {
    individual_lock: LockClass,
    aggregate_lock: LockClass,
}

impl ConvictionModel {
    pub fn new(
        owner_lock: bool,
        perpetual_lock: bool,
        individual_lock_state: LockState,
        aggregate_lock_state: LockState,
    ) -> Self {
        Self {
            individual_lock: LockClass::new(individual_lock_state, owner_lock, perpetual_lock),
            aggregate_lock: LockClass::new(aggregate_lock_state, owner_lock, perpetual_lock),
        }
    }

    pub fn individual_lock(&self) -> &LockState {
        self.individual_lock.lock()
    }

    pub fn aggregate_lock(&self) -> &LockState {
        self.aggregate_lock.lock()
    }

    pub fn rolled_individual(&self, now: u64, unlock_rate: u64, maturity_rate: u64) -> LockState {
        self.individual_lock
            .roll_forward(now, unlock_rate, maturity_rate)
            .into_lock()
            .normalize_dust()
    }

    pub fn merge(&mut self, conv: &ConvictionModel) {
        match (
            &self.individual_lock,
            &self.aggregate_lock,
            &conv.individual_lock,
            &conv.aggregate_lock,
        ) {
            (
                LockClass::PerpetualGeneral(_),
                LockClass::PerpetualGeneral(_),
                LockClass::PerpetualGeneral(_),
                LockClass::PerpetualGeneral(_),
            )
            | (
                LockClass::DecayingGeneral(_),
                LockClass::DecayingGeneral(_),
                LockClass::DecayingGeneral(_),
                LockClass::DecayingGeneral(_),
            )
            | (
                LockClass::PerpetualOwner(_),
                LockClass::PerpetualOwner(_),
                LockClass::PerpetualOwner(_),
                LockClass::PerpetualOwner(_),
            )
            | (
                LockClass::DecayingOwner(_),
                LockClass::DecayingOwner(_),
                LockClass::DecayingOwner(_),
                LockClass::DecayingOwner(_),
            ) => {}
            _ => {
                log::error!("Cannot merge conviction models with different lock classes");
                return;
            }
        }

        let individual = self.individual_lock.lock().add(conv.individual_lock.lock());
        let aggregate = self.aggregate_lock.lock().add(conv.aggregate_lock.lock());
        *self.individual_lock.lock_mut() = individual;
        *self.aggregate_lock.lock_mut() = aggregate;
    }

    /// Rolls the individual lock and its aggregate bucket forward together.
    /// If individual lock becomes dust, makes it zero and removes it from the aggregate.
    pub fn roll_forward(&mut self, now: u64, unlock_rate: u64, maturity_rate: u64) {
        if self.individual_lock.flags() != self.aggregate_lock.flags() {
            log::error!(
                "Cannot roll conviction model with different individual and aggregate classes"
            );
            return;
        }

        self.individual_lock = self
            .individual_lock
            .roll_forward(now, unlock_rate, maturity_rate);
        self.aggregate_lock = self
            .aggregate_lock
            .roll_forward(now, unlock_rate, maturity_rate);

        self.collect_individual_dust();
    }

    /// Rolls the model forward and adds locked mass while keeping the individual and
    /// aggregate contributions synchronized.
    fn add_locked_mass(
        &mut self,
        amount: AlphaBalance,
        now: u64,
        unlock_rate: u64,
        maturity_rate: u64,
    ) {
        self.roll_forward(now, unlock_rate, maturity_rate);

        let owner_lock = match (&self.individual_lock, &self.aggregate_lock) {
            (LockClass::PerpetualGeneral(_), LockClass::PerpetualGeneral(_))
            | (LockClass::DecayingGeneral(_), LockClass::DecayingGeneral(_)) => false,
            (LockClass::PerpetualOwner(_), LockClass::PerpetualOwner(_))
            | (LockClass::DecayingOwner(_), LockClass::DecayingOwner(_)) => true,
            _ => {
                log::error!("Cannot add locked mass to different individual and aggregate classes");
                return;
            }
        };

        let individual = self.individual_lock.lock_mut();
        let aggregate = self.aggregate_lock.lock_mut();
        individual.locked_mass = individual.locked_mass.saturating_add(amount);
        aggregate.locked_mass = aggregate.locked_mass.saturating_add(amount);
        if owner_lock {
            individual.conviction = U64F64::saturating_from_num(u64::from(individual.locked_mass));
            aggregate.conviction = U64F64::saturating_from_num(u64::from(aggregate.locked_mass));
        }

        self.collect_individual_dust();
    }

    fn force_reduce_individual(&mut self, amount: AlphaBalance, now: u64) {
        if self.individual_lock.flags() != self.aggregate_lock.flags() {
            log::error!("Cannot reduce lock with different individual and aggregate classes");
            return;
        }

        let before = self.individual_lock.lock().clone();
        let new_locked_mass = before.locked_mass.saturating_sub(amount);
        let new_conviction = if new_locked_mass.is_zero() {
            U64F64::saturating_from_num(0)
        } else {
            let remaining = U64F64::saturating_from_num(u64::from(new_locked_mass))
                .safe_div(U64F64::saturating_from_num(u64::from(before.locked_mass)));
            before.conviction.saturating_mul(remaining)
        };

        let individual = self.individual_lock.lock_mut();
        individual.locked_mass = new_locked_mass;
        individual.conviction = new_conviction;
        individual.last_update = now;

        let aggregate = self.aggregate_lock.lock_mut();
        aggregate.locked_mass = aggregate
            .locked_mass
            .saturating_sub(before.locked_mass.saturating_sub(new_locked_mass));
        aggregate.conviction = aggregate
            .conviction
            .saturating_sub(before.conviction.saturating_sub(new_conviction));

        self.collect_individual_dust();
    }

    fn collect_individual_dust(&mut self) {
        if !self.individual_lock.lock().is_dust() {
            return;
        }

        let dust = self.individual_lock.lock().clone();
        let aggregate = self.aggregate_lock.lock_mut();
        aggregate.locked_mass = aggregate.locked_mass.saturating_sub(dust.locked_mass);
        aggregate.conviction = aggregate.conviction.saturating_sub(dust.conviction);

        let individual = self.individual_lock.lock_mut();
        individual.locked_mass = AlphaBalance::ZERO;
        individual.conviction = U64F64::saturating_from_num(0);
    }

    fn roll_forward_aggregate(&mut self, now: u64, unlock_rate: u64, maturity_rate: u64) {
        self.aggregate_lock = self
            .aggregate_lock
            .roll_forward(now, unlock_rate, maturity_rate);
    }

    fn cloned(&self) -> Self {
        Self {
            individual_lock: self.individual_lock.clone(),
            aggregate_lock: self.aggregate_lock.clone(),
        }
    }

    fn remove_individual_contribution(&mut self) -> LockState {
        let contribution = self.individual_lock.lock().clone();
        let aggregate = self.aggregate_lock.lock_mut();
        aggregate.locked_mass = aggregate
            .locked_mass
            .saturating_sub(contribution.locked_mass);
        aggregate.conviction = aggregate.conviction.saturating_sub(contribution.conviction);
        *self.individual_lock.lock_mut() = LockState {
            locked_mass: AlphaBalance::ZERO,
            conviction: U64F64::saturating_from_num(0),
            last_update: contribution.last_update,
        };
        contribution
    }

    fn replace_individual(&mut self, replacement: LockState) {
        let previous = self.individual_lock.lock().clone();
        let aggregate = self.aggregate_lock.lock_mut();
        aggregate.locked_mass = aggregate
            .locked_mass
            .saturating_sub(previous.locked_mass)
            .saturating_add(replacement.locked_mass);
        aggregate.conviction = aggregate
            .conviction
            .saturating_sub(previous.conviction)
            .saturating_add(replacement.conviction);
        *self.individual_lock.lock_mut() = replacement;
    }

    pub fn set_perpetual(&mut self, perpetual: bool) -> Self {
        let individual_flags = self.individual_lock.flags();
        if individual_flags != self.aggregate_lock.flags() {
            log::error!(
                "Cannot change perpetual behavior for different individual and aggregate classes"
            );
            return self.cloned();
        }

        let (owner, currently_perpetual) = individual_flags;
        if currently_perpetual == perpetual {
            return self.cloned();
        }

        let contribution = self.remove_individual_contribution();
        ConvictionModel::new(owner, perpetual, contribution.clone(), contribution)
    }

    pub fn set_owner(&mut self, owner: bool) -> Self {
        let individual_flags = self.individual_lock.flags();
        if individual_flags != self.aggregate_lock.flags() {
            log::error!(
                "Cannot change owner behavior for different individual and aggregate classes"
            );
            return self.cloned();
        }

        let (currently_owner, perpetual) = individual_flags;
        if currently_owner == owner {
            return self.cloned();
        }

        let mut contribution = self.remove_individual_contribution();
        if owner {
            contribution.conviction =
                U64F64::saturating_from_num(u64::from(contribution.locked_mass));
        }

        ConvictionModel::new(owner, perpetual, contribution.clone(), contribution)
    }

    fn aggregate_mut(&mut self) -> &mut LockState {
        self.aggregate_lock.lock_mut()
    }
}

impl<T: Config> Pallet<T> {
    pub fn add_locking_coldkey(hotkey: &T::AccountId, netuid: NetUid, coldkey: &T::AccountId) {
        if LockingColdkeys::<T>::contains_key((netuid, hotkey, coldkey)) {
            return;
        }
        LockingColdkeys::<T>::insert((netuid, hotkey, coldkey), ());
    }

    pub fn maybe_remove_locking_coldkey(
        hotkey: &T::AccountId,
        netuid: NetUid,
        coldkey: &T::AccountId,
    ) {
        let _ = LockingColdkeys::<T>::take((netuid, hotkey, coldkey));
    }

    /// Number of indexed individual locks touched by an ownership transition.
    ///
    /// REVIEW NOTE: This scan is intentionally unbounded. Ownership transitions are
    /// exceptionally rare (expected only a few times per year), and the mainnet scan
    /// performed for the conviction migration found at most 44 lock rows on one subnet.
    /// Reviewers should not request a maintained counter or staged transition solely
    /// because this iterator is unbounded; the deliberate tradeoff is to charge the
    /// complete member-scaled work instead of adding permanent storage bookkeeping.
    pub fn owner_transition_member_count(netuid: NetUid, new_owner_hotkey: &T::AccountId) -> u32 {
        let old_owner_hotkey = SubnetOwnerHotkey::<T>::get(netuid);
        let old_owner_members = LockingColdkeys::<T>::iter_prefix((netuid, &old_owner_hotkey))
            .fold(0u32, |count, _| count.saturating_add(1));

        if &old_owner_hotkey == new_owner_hotkey {
            return old_owner_members;
        }

        LockingColdkeys::<T>::iter_prefix((netuid, new_owner_hotkey))
            .fold(old_owner_members, |count, _| count.saturating_add(1))
    }

    /// Database cost of looking up an ownership transition's member count before dispatch.
    pub fn owner_transition_member_count_weight(member_count: u32) -> Weight {
        // One SubnetOwnerHotkey read, one read per indexed member, and one terminal
        // read for each of the two LockingColdkeys prefix iterators. This slightly
        // overcharges the no-op same-hotkey case, which only runs one iterator.
        T::DbWeight::get().reads(u64::from(member_count).saturating_add(3))
    }

    /// Number of indexed individual locks touched when terminating a lease.
    pub fn lease_owner_transition_member_count(
        lease_id: LeaseId,
        new_owner_hotkey: &T::AccountId,
    ) -> u32 {
        SubnetLeases::<T>::get(lease_id)
            .map(|lease| Self::owner_transition_member_count(lease.netuid, new_owner_hotkey))
            .unwrap_or(0)
    }

    /// Database cost of looking up a lease ownership transition's member count before dispatch.
    pub fn lease_owner_transition_member_count_weight(member_count: u32) -> Weight {
        // SubnetLeases plus the ordinary ownership-transition count lookup.
        T::DbWeight::get().reads(u64::from(member_count).saturating_add(4))
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
        if lock_state.is_dust() {
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
        let owner_lock = Self::is_subnet_owner_hotkey(netuid, hotkey);
        let perpetual_lock = Self::is_perpetual_lock(coldkey, netuid);
        Self::read_conviction_model_for_class(
            coldkey,
            netuid,
            hotkey,
            now,
            owner_lock,
            perpetual_lock,
        )
    }

    fn read_conviction_model_for_class(
        coldkey: &T::AccountId,
        netuid: NetUid,
        hotkey: &T::AccountId,
        now: u64,
        owner_lock: bool,
        perpetual_lock: bool,
    ) -> ConvictionModel {
        let aggregate_lock = match (owner_lock, perpetual_lock) {
            (false, true) => {
                HotkeyLock::<T>::get(netuid, hotkey).unwrap_or_else(|| Self::empty_lock(now))
            }
            (false, false) => DecayingHotkeyLock::<T>::get(netuid, hotkey)
                .unwrap_or_else(|| Self::empty_lock(now)),
            (true, true) => OwnerLock::<T>::get(netuid).unwrap_or_else(|| Self::empty_lock(now)),
            (true, false) => {
                DecayingOwnerLock::<T>::get(netuid).unwrap_or_else(|| Self::empty_lock(now))
            }
        };

        ConvictionModel::new(
            owner_lock,
            perpetual_lock,
            Lock::<T>::get((coldkey, netuid, hotkey)).unwrap_or_else(|| Self::empty_lock(now)),
            aggregate_lock,
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
        Self::insert_lock_state(coldkey, netuid, hotkey, model.individual_lock().clone());

        match model.aggregate_lock {
            LockClass::PerpetualGeneral(aggregate) => {
                Self::insert_hotkey_lock_state(netuid, hotkey, aggregate.lock);
            }
            LockClass::DecayingGeneral(aggregate) => {
                Self::insert_decaying_hotkey_lock_state(netuid, hotkey, aggregate.lock);
            }
            LockClass::PerpetualOwner(aggregate) => {
                Self::insert_owner_lock_state(netuid, aggregate.lock);
            }
            LockClass::DecayingOwner(aggregate) => {
                Self::insert_decaying_owner_lock_state(netuid, aggregate.lock);
            }
        }
    }

    pub fn do_set_perpetual_lock(
        coldkey: &T::AccountId,
        netuid: NetUid,
        enabled: bool,
    ) -> DispatchResult {
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);

        let now = Self::get_current_block_as_u64();
        let unlock_rate = UnlockRate::<T>::get();
        let maturity_rate = MaturityRate::<T>::get();
        let current_enabled = Self::is_perpetual_lock(coldkey, netuid);

        let reclassified_model = if current_enabled == enabled {
            if let Some((hotkey, mut model)) = Self::read_conviction_model(coldkey, netuid, now) {
                model.roll_forward(now, unlock_rate, maturity_rate);
                Self::save_conviction_model(coldkey, netuid, &hotkey, model);
            }
            None
        } else {
            Self::read_conviction_model(coldkey, netuid, now).map(|(hotkey, mut model)| {
                model.roll_forward(now, unlock_rate, maturity_rate);
                let reclassified = model.set_perpetual(enabled);
                Self::save_conviction_model(coldkey, netuid, &hotkey, model);
                (hotkey, reclassified)
            })
        };

        if enabled {
            DecayingLock::<T>::insert(coldkey, netuid, false);
        } else {
            DecayingLock::<T>::remove(coldkey, netuid);
        }

        if let Some((hotkey, reclassified)) = reclassified_model {
            let mut destination =
                Self::read_conviction_model_for_hotkey(coldkey, netuid, &hotkey, now);
            destination.roll_forward(now, unlock_rate, maturity_rate);
            destination.merge(&reclassified);
            Self::save_conviction_model(coldkey, netuid, &hotkey, destination);
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
            .map(|(_hotkey, model)| {
                model
                    .rolled_individual(now, UnlockRate::<T>::get(), MaturityRate::<T>::get())
                    .locked_mass
            })
            .unwrap_or(AlphaBalance::ZERO)
    }

    /// Returns the current conviction for a coldkey on a subnet (rolled forward to now).
    pub fn get_conviction(coldkey: &T::AccountId, netuid: NetUid) -> U64F64 {
        let now = Self::get_current_block_as_u64();
        Self::read_conviction_model(coldkey, netuid, now)
            .map(|(_hotkey, model)| {
                model
                    .rolled_individual(now, UnlockRate::<T>::get(), MaturityRate::<T>::get())
                    .conviction
            })
            .unwrap_or_else(|| U64F64::saturating_from_num(0))
    }

    /// Returns the current lock for a coldkey on a subnet, rolled forward to now.
    pub fn get_coldkey_lock(coldkey: &T::AccountId, netuid: NetUid) -> Option<LockState> {
        let now = Self::get_current_block_as_u64();
        Self::read_conviction_model(coldkey, netuid, now).map(|(_hotkey, model)| {
            model.rolled_individual(now, UnlockRate::<T>::get(), MaturityRate::<T>::get())
        })
    }

    /// (total_stake, locked_mass, available_to_unstake) for a coldkey on one subnet.
    ///
    /// The conviction lock is subnet-wide: it blocks unstaking from any hotkey on
    /// that subnet, not from a single hotkey position. Miner registration
    /// collateral is also subtracted here as a coldkey-wide residual; call sites
    /// that know the origin hotkey must additionally call
    /// `ensure_hotkey_covers_collateral` so the bond cannot be covered by free
    /// stake on a sibling hotkey.
    pub fn stake_availability(
        coldkey: &T::AccountId,
        netuid: NetUid,
    ) -> (AlphaBalance, AlphaBalance, AlphaBalance) {
        let total = Self::total_coldkey_alpha_on_subnet(coldkey, netuid);
        let locked = Self::get_current_locked(coldkey, netuid);
        let collateral = Self::total_miner_collateral_for_coldkey(coldkey, netuid);
        let available = total.saturating_sub(locked).saturating_sub(collateral);
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
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
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
        } else {
            ensure!(
                total >= model.individual_lock().locked_mass.saturating_add(amount),
                Error::<T>::InsufficientStakeForLock
            );
        }

        model.add_locked_mass(
            amount,
            now,
            UnlockRate::<T>::get(),
            MaturityRate::<T>::get(),
        );
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
        let aggregate = model.aggregate_mut();
        aggregate.locked_mass = aggregate.locked_mass.saturating_add(added.locked_mass);
        aggregate.conviction = aggregate.conviction.saturating_add(added.conviction);
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
        let aggregate = model.aggregate_mut();
        aggregate.locked_mass = aggregate.locked_mass.saturating_sub(amount);
        aggregate.conviction = aggregate.conviction.saturating_sub(conviction);
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
                roll_lock_state(lock, now, unlock_rate, maturity_rate, false, true).conviction
            })
            .unwrap_or_else(|| U64F64::saturating_from_num(0));
        let decaying_conviction = DecayingHotkeyLock::<T>::get(netuid, hotkey)
            .map(|lock| {
                roll_lock_state(lock, now, unlock_rate, maturity_rate, false, false).conviction
            })
            .unwrap_or_else(|| U64F64::saturating_from_num(0));
        let hotkey_conviction = perpetual_conviction.saturating_add(decaying_conviction);
        if hotkey == &SubnetOwnerHotkey::<T>::get(netuid) {
            let owner_conviction = OwnerLock::<T>::get(netuid)
                .map(|lock| {
                    roll_lock_state(lock, now, unlock_rate, maturity_rate, true, true).conviction
                })
                .unwrap_or_else(|| U64F64::saturating_from_num(0));
            let decaying_owner_conviction = DecayingOwnerLock::<T>::get(netuid)
                .map(|lock| {
                    roll_lock_state(lock, now, unlock_rate, maturity_rate, true, false).conviction
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
                roll_lock_state(lock, now, unlock_rate, maturity_rate, false, true).conviction
            })
            .fold(U64F64::saturating_from_num(0), |acc, conviction| {
                acc.saturating_add(conviction)
            });
        let decaying_hotkey_conviction = DecayingHotkeyLock::<T>::iter_prefix(netuid)
            .map(|(_hotkey, lock)| {
                roll_lock_state(lock, now, unlock_rate, maturity_rate, false, false).conviction
            })
            .fold(U64F64::saturating_from_num(0), |acc, conviction| {
                acc.saturating_add(conviction)
            });
        let owner_conviction = OwnerLock::<T>::get(netuid)
            .map(|lock| {
                roll_lock_state(lock, now, unlock_rate, maturity_rate, true, true).conviction
            })
            .unwrap_or_else(|| U64F64::saturating_from_num(0));
        let decaying_owner_conviction = DecayingOwnerLock::<T>::get(netuid)
            .map(|lock| {
                roll_lock_state(lock, now, unlock_rate, maturity_rate, true, false).conviction
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
            let rolled = roll_lock_state(lock, now, unlock_rate, maturity_rate, false, true);
            let entry = scores
                .entry(hotkey)
                .or_insert_with(|| U64F64::saturating_from_num(0));
            *entry = entry.saturating_add(rolled.conviction);
        });
        DecayingHotkeyLock::<T>::iter_prefix(netuid).for_each(|(hotkey, lock)| {
            let rolled = roll_lock_state(lock, now, unlock_rate, maturity_rate, false, false);
            let entry = scores
                .entry(hotkey)
                .or_insert_with(|| U64F64::saturating_from_num(0));
            *entry = entry.saturating_add(rolled.conviction);
        });
        if let Some(lock) = OwnerLock::<T>::get(netuid) {
            let owner_hotkey = SubnetOwnerHotkey::<T>::get(netuid);
            let rolled = roll_lock_state(lock, now, unlock_rate, maturity_rate, true, true);
            let entry = scores
                .entry(owner_hotkey)
                .or_insert_with(|| U64F64::saturating_from_num(0));
            *entry = entry.saturating_add(rolled.conviction);
        }
        if let Some(lock) = DecayingOwnerLock::<T>::get(netuid) {
            let owner_hotkey = SubnetOwnerHotkey::<T>::get(netuid);
            let rolled = roll_lock_state(lock, now, unlock_rate, maturity_rate, true, false);
            let entry = scores
                .entry(owner_hotkey)
                .or_insert_with(|| U64F64::saturating_from_num(0));
            *entry = entry.saturating_add(rolled.conviction);
        }

        scores
            .into_iter()
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(core::cmp::Ordering::Equal))
            .map(|(hotkey, _)| hotkey)
    }

    fn transition_hotkey_lock_owner_class(
        netuid: NetUid,
        hotkey: &T::AccountId,
        previous_owner: bool,
        new_owner: bool,
        now: u64,
        unlock_rate: u64,
        maturity_rate: u64,
    ) -> u32 {
        // Intentionally unbounded: ownership changes are rare, and the full
        // member-scaled work is charged. See owner_transition_member_count for
        // the operational rationale and observed mainnet size.
        let coldkeys: Vec<T::AccountId> = LockingColdkeys::<T>::iter_prefix((netuid, hotkey))
            .map(|(coldkey, ())| coldkey)
            .collect();
        let mut transitioned = 0u32;

        for coldkey in coldkeys {
            if !Lock::<T>::contains_key((&coldkey, netuid, hotkey)) {
                Self::maybe_remove_locking_coldkey(hotkey, netuid, &coldkey);
                continue;
            }

            let perpetual_lock = Self::is_perpetual_lock(&coldkey, netuid);
            let mut source = Self::read_conviction_model_for_class(
                &coldkey,
                netuid,
                hotkey,
                now,
                previous_owner,
                perpetual_lock,
            );
            source.roll_forward(now, unlock_rate, maturity_rate);
            let contribution = source.set_owner(new_owner);
            Self::save_conviction_model(&coldkey, netuid, hotkey, source);

            let mut destination = Self::read_conviction_model_for_class(
                &coldkey,
                netuid,
                hotkey,
                now,
                new_owner,
                perpetual_lock,
            );
            destination.roll_forward(now, unlock_rate, maturity_rate);
            destination.merge(&contribution);
            Self::save_conviction_model(&coldkey, netuid, hotkey, destination);

            transitioned = transitioned.saturating_add(1);
        }

        transitioned
    }

    /// Reclassify canonical individual locks and their aggregate buckets when a
    /// subnet's owner hotkey changes.
    ///
    /// This must run before updating [`SubnetOwnerHotkey`]. Every member of the
    /// outgoing and incoming hotkeys is rolled to now and moved between the
    /// corresponding aggregate classes. Keeping the owner boost on both
    /// representations prevents it becoming orphaned after demotion.
    pub(crate) fn transition_subnet_owner_lock_aggregates(
        netuid: NetUid,
        old_owner_hotkey: &T::AccountId,
        new_owner_hotkey: &T::AccountId,
    ) -> u32 {
        let now = Self::get_current_block_as_u64();
        let unlock_rate = UnlockRate::<T>::get();
        let maturity_rate = MaturityRate::<T>::get();
        if old_owner_hotkey == new_owner_hotkey {
            return 0;
        }

        let demoted = Self::transition_hotkey_lock_owner_class(
            netuid,
            old_owner_hotkey,
            true,
            false,
            now,
            unlock_rate,
            maturity_rate,
        );
        let promoted = Self::transition_hotkey_lock_owner_class(
            netuid,
            new_owner_hotkey,
            false,
            true,
            now,
            unlock_rate,
            maturity_rate,
        );

        demoted.saturating_add(promoted)
    }

    /// Reassigns subnet ownership to the current lock-conviction leader when the subnet
    /// is mature enough and enough conviction has accumulated.
    ///
    /// Ownership can change only after the subnet is at least [`ONE_YEAR`] old and the
    /// hotkey with the highest rolled aggregate conviction itself holds at least 10% of
    /// `SubnetAlphaOut`. If those gates pass, that hotkey becomes the subnet owner
    /// hotkey, and its owning coldkey becomes the subnet owner coldkey. The new owner
    /// hotkey's conviction is then progressed to its current locked mass so the new
    /// owner starts with full owner conviction.
    pub fn change_subnet_owner_if_needed(netuid: NetUid) -> Weight {
        // No outstanding alpha means there is no meaningful 10% conviction threshold.
        let subnet_alpha_out = SubnetAlphaOut::<T>::get(netuid);
        if subnet_alpha_out.is_zero() {
            return Weight::zero();
        }

        // Ownership can only be reassigned after the subnet has aged for one year.
        let now = Self::get_current_block_as_u64();
        let registered_at = NetworkRegisteredAt::<T>::get(netuid);
        if now < registered_at.saturating_add(ONE_YEAR) {
            return Weight::zero();
        }

        // Pick the hotkey with the highest rolled aggregate conviction.
        let Some(king_hotkey) = Self::subnet_king(netuid) else {
            return Weight::zero();
        };

        // The challenger must itself hold at least 10% of subnet alpha out.
        // Gating on subnet-wide conviction would let unrelated lockers,
        // including the incumbent, supply the challenger's quorum.
        let king_conviction = Self::hotkey_conviction(&king_hotkey, netuid);
        if king_conviction.saturating_mul(U64F64::saturating_from_num(10))
            < U64F64::saturating_from_num(u64::from(subnet_alpha_out))
        {
            return Weight::zero();
        }

        // The king hotkey must resolve to a real coldkey owner.
        let new_owner_coldkey = Self::get_owning_coldkey_for_hotkey(&king_hotkey);
        if new_owner_coldkey == DefaultAccount::<T>::get() {
            return Weight::zero();
        }

        // If the winning hotkey already belongs to the current owner, nothing changes.
        let current_owner_coldkey = SubnetOwner::<T>::get(netuid);
        if new_owner_coldkey == current_owner_coldkey {
            return Weight::zero();
        }
        let old_owner_hotkey = SubnetOwnerHotkey::<T>::get(netuid);

        // Register new owner as a neuron if not yet registered.
        if Self::get_uid_for_net_and_hotkey(netuid, &king_hotkey).is_err()
            && Self::register_neuron(netuid, &king_hotkey).is_err()
        {
            return Weight::zero();
        }

        let transitioned_members =
            Self::transition_subnet_owner_lock_aggregates(netuid, &old_owner_hotkey, &king_hotkey);

        // Reassign subnet owner coldkey and owner hotkey.
        SubnetOwner::<T>::insert(netuid, new_owner_coldkey.clone());
        SubnetOwnerHotkey::<T>::insert(netuid, king_hotkey.clone());
        Self::deposit_event(Event::SubnetOwnerChanged {
            netuid,
            old_coldkey: current_owner_coldkey,
            new_coldkey: new_owner_coldkey,
        });

        <T as Config>::WeightInfo::transition_subnet_owner_locks(transitioned_members)
    }

    /// Ensure the coldkey does not have an active lock on any subnets.
    pub fn ensure_no_active_locks(coldkey: &T::AccountId) -> Result<(), Error<T>> {
        let now = Self::get_current_block_as_u64();
        let unlock_rate = UnlockRate::<T>::get();
        let maturity_rate = MaturityRate::<T>::get();

        for ((netuid, hotkey), lock) in Lock::<T>::iter_prefix((coldkey,)) {
            let rolled = roll_lock_state(
                lock,
                now,
                unlock_rate,
                maturity_rate,
                Self::is_subnet_owner_hotkey(netuid, &hotkey),
                Self::is_perpetual_lock(coldkey, netuid),
            );
            if rolled.locked_mass > AlphaBalance::ZERO {
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
            let old_lock = roll_lock_state(
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
            let new_lock = roll_lock_state(
                old_lock.clone(),
                now,
                unlock_rate,
                maturity_rate,
                Self::is_subnet_owner_hotkey(netuid, &hotkey),
                perpetual_lock,
            );
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
            let rolled = roll_lock_state(
                lock,
                now,
                unlock_rate,
                maturity_rate,
                old_owner_lock,
                perpetual_lock,
            );
            let moved = roll_lock_state(
                rolled,
                now,
                unlock_rate,
                maturity_rate,
                new_owner_lock,
                perpetual_lock,
            );
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
                OwnerLock::<T>::take(netuid)
                    .map(|lock| roll_lock_state(lock, now, unlock_rate, maturity_rate, true, true))
            } else {
                HotkeyLock::<T>::take(netuid, old_hotkey)
                    .map(|lock| roll_lock_state(lock, now, unlock_rate, maturity_rate, false, true))
            };
            let moved_decaying_lock = if old_was_owner {
                DecayingOwnerLock::<T>::take(netuid)
                    .map(|lock| roll_lock_state(lock, now, unlock_rate, maturity_rate, true, false))
            } else {
                DecayingHotkeyLock::<T>::take(netuid, old_hotkey).map(|lock| {
                    roll_lock_state(lock, now, unlock_rate, maturity_rate, false, false)
                })
            };

            if let Some(lock) = moved_perpetual_lock {
                if new_is_owner {
                    Self::insert_owner_lock_state(
                        netuid,
                        roll_lock_state(lock, now, unlock_rate, maturity_rate, true, true),
                    );
                } else {
                    Self::insert_hotkey_lock_state(
                        netuid,
                        new_hotkey,
                        roll_lock_state(lock, now, unlock_rate, maturity_rate, false, true),
                    );
                }
            }
            if let Some(lock) = moved_decaying_lock {
                if new_is_owner {
                    Self::insert_decaying_owner_lock_state(
                        netuid,
                        roll_lock_state(lock, now, unlock_rate, maturity_rate, true, false),
                    );
                } else {
                    Self::insert_decaying_hotkey_lock_state(
                        netuid,
                        new_hotkey,
                        roll_lock_state(lock, now, unlock_rate, maturity_rate, false, false),
                    );
                }
            }
            writes = writes.saturating_add(6);
        }
        (reads, writes)
    }

    /// Conviction is only preserved when a lock moves between hotkeys owned by
    /// the same coldkey; moving it to a differently owned hotkey forfeits it.
    /// Shared by `do_move_lock` and `transfer_lock`.
    fn conviction_survives_hotkey_change(
        source_hotkey: &T::AccountId,
        destination_hotkey: &T::AccountId,
    ) -> bool {
        Self::get_owning_coldkey_for_hotkey(source_hotkey)
            == Self::get_owning_coldkey_for_hotkey(destination_hotkey)
    }

    /// Saves a synchronized source model before re-reading and mutating the
    /// destination model.
    ///
    /// Source and destination can share an aggregate bucket. Re-reading after
    /// the source save prevents a stale destination snapshot from restoring a
    /// contribution that the source mutation just removed.
    fn save_source_then_update_destination<F>(
        source_coldkey: &T::AccountId,
        source_hotkey: &T::AccountId,
        source_model: ConvictionModel,
        destination_coldkey: &T::AccountId,
        destination_hotkey: &T::AccountId,
        netuid: NetUid,
        now: u64,
        unlock_rate: u64,
        maturity_rate: u64,
        update_destination: F,
    ) where
        F: FnOnce(&mut ConvictionModel),
    {
        Self::save_conviction_model(source_coldkey, netuid, source_hotkey, source_model);

        let mut destination_model = Self::read_conviction_model_for_hotkey(
            destination_coldkey,
            netuid,
            destination_hotkey,
            now,
        );
        destination_model.roll_forward(now, unlock_rate, maturity_rate);
        update_destination(&mut destination_model);
        Self::save_conviction_model(
            destination_coldkey,
            netuid,
            destination_hotkey,
            destination_model,
        );
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
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
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
                let mut lock = model.remove_individual_contribution();

                if !Self::conviction_survives_hotkey_change(&origin_hotkey, destination_hotkey) {
                    lock.conviction = U64F64::saturating_from_num(0);
                }
                lock = roll_lock_state(
                    lock,
                    now,
                    unlock_rate,
                    maturity_rate,
                    Self::is_subnet_owner_hotkey(netuid, destination_hotkey),
                    Self::is_perpetual_lock(coldkey, netuid),
                );

                Self::save_source_then_update_destination(
                    coldkey,
                    &origin_hotkey,
                    model,
                    coldkey,
                    destination_hotkey,
                    netuid,
                    now,
                    unlock_rate,
                    maturity_rate,
                    move |destination_model| {
                        let combined = destination_model.individual_lock().add(&lock);
                        destination_model.replace_individual(combined);
                    },
                );

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
    /// coldkey is locked in the same way as the original stake: the lock follows the stake
    /// to `destination_hotkey` (which, for plain stake transfers, is the same hotkey the
    /// stake was locked to). Conviction is moved proportionally to the moved locked amount
    /// of alpha. For example, if 20% of locked alpha is moved, then also 20% of conviction
    /// is moved. If the source and destination hotkeys are owned by different coldkeys,
    /// the moved conviction is reset to zero, mirroring `do_move_lock`.
    pub fn transfer_lock(
        origin_coldkey: &T::AccountId,
        destination_coldkey: &T::AccountId,
        destination_hotkey: &T::AccountId,
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
        let maybe_destination_hotkey =
            Self::read_conviction_model(destination_coldkey, netuid, now)
                .map(|(hotkey, _model)| hotkey);

        let destination_lock_hotkey = maybe_destination_hotkey
            .as_ref()
            .cloned()
            .unwrap_or_else(|| destination_hotkey.clone());

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
        let mut received_conviction = U64F64::saturating_from_num(0);
        if !remaining_to_transfer.is_zero() {
            if let Some(existing_hotkey) = maybe_destination_hotkey.as_ref() {
                ensure!(
                    existing_hotkey == destination_hotkey,
                    Error::<T>::LockHotkeyMismatch
                );
            }

            locked_transfer = remaining_to_transfer.min(source_lock.locked_mass);
            let conviction_transfer = if locked_transfer.is_zero()
                || source_lock.locked_mass.is_zero()
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

            // Conviction only follows the lock when the destination hotkey is owned
            // by the same coldkey as the source hotkey; otherwise it is forfeited,
            // mirroring `do_move_lock`.
            received_conviction = if Self::conviction_survives_hotkey_change(
                &source_hotkey,
                &destination_lock_hotkey,
            ) {
                conviction_transfer
            } else {
                U64F64::saturating_from_num(0)
            };

            source_lock.locked_mass = source_lock.locked_mass.saturating_sub(locked_transfer);
            source_lock.conviction = source_lock.conviction.saturating_sub(conviction_transfer);
        }
        Self::ensure_can_receive_locked_alpha(destination_coldkey, locked_transfer)?;

        source_lock = roll_lock_state(
            source_lock,
            now,
            unlock_rate,
            maturity_rate,
            Self::is_subnet_owner_hotkey(netuid, &source_hotkey),
            Self::is_perpetual_lock(origin_coldkey, netuid),
        );
        source_model.replace_individual(source_lock);

        if !locked_transfer.is_zero() || maybe_destination_hotkey.is_some() {
            let destination_owner_hotkey = destination_lock_hotkey.clone();
            Self::save_source_then_update_destination(
                origin_coldkey,
                &source_hotkey,
                source_model,
                destination_coldkey,
                &destination_lock_hotkey,
                netuid,
                now,
                unlock_rate,
                maturity_rate,
                move |destination_model| {
                    let mut destination_lock = destination_model.individual_lock().clone();
                    destination_lock.locked_mass =
                        destination_lock.locked_mass.saturating_add(locked_transfer);
                    destination_lock.conviction = destination_lock
                        .conviction
                        .saturating_add(received_conviction);
                    destination_lock = roll_lock_state(
                        destination_lock,
                        now,
                        unlock_rate,
                        maturity_rate,
                        Self::is_subnet_owner_hotkey(netuid, &destination_owner_hotkey),
                        Self::is_perpetual_lock(destination_coldkey, netuid),
                    );
                    destination_model.replace_individual(destination_lock);
                },
            );
        } else {
            Self::save_conviction_model(origin_coldkey, netuid, &source_hotkey, source_model);
        }

        Ok(())
    }

    /// Removes `Lock` entries for `netuid`, resuming from `LastKeptRawKey` when weight is limited.
    pub fn remove_network_lock(
        netuid: NetUid,
        weight_meter: &mut WeightMeter,
        last_key: Option<Vec<u8>>,
    ) -> (bool, Option<Vec<u8>>) {
        let iter = match last_key {
            Some(key) => Lock::<T>::iter_from(key),
            None => Lock::<T>::iter(),
        };

        let (read_all, last_item) = Self::remove_storage_entries_for_netuid(
            weight_meter,
            iter,
            |((_, this_netuid, _), _)| *this_netuid == netuid,
            |((coldkey, _this_netuid, hotkey), _)| (coldkey, hotkey),
            |(coldkey, hotkey)| Lock::<T>::remove((coldkey.clone(), netuid, hotkey.clone())),
            1,
        );

        (
            read_all,
            last_item.map(|((coldkey, _, hotkey), _)| {
                Lock::<T>::hashed_key_for((&coldkey, netuid, &hotkey))
            }),
        )
    }

    /// Removes `DecayingLock` entries for `netuid`, resuming from `LastKeptRawKey` when weight is limited.
    pub fn remove_network_decaying_lock(
        netuid: NetUid,
        weight_meter: &mut WeightMeter,
        last_key: Option<Vec<u8>>,
    ) -> (bool, Option<Vec<u8>>) {
        let iter = match last_key {
            Some(raw_key) => DecayingLock::<T>::iter_from(raw_key),
            None => DecayingLock::<T>::iter(),
        };

        let (read_all, last_item) = Self::remove_storage_entries_for_netuid(
            weight_meter,
            iter,
            |(_, nu, _)| *nu == netuid,
            |(cold, nu, _)| (cold, nu),
            |(cold, netuid)| DecayingLock::<T>::remove(cold, netuid),
            1,
        );

        (
            read_all,
            last_item.map(|(cold, nu, _)| DecayingLock::<T>::hashed_key_for(&cold, nu)),
        )
    }
}
