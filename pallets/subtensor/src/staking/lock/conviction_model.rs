//! Conviction lock types: [`LockState`], [`RollDelta`], and [`ConvictionModel`].
//!
//! These types model exponentially decaying locked alpha and its matured
//! conviction score. Aggregate buckets (owner vs general, perpetual vs decaying)
//! are updated together so hotkey / owner totals stay consistent with individuals.
use super::*;
use codec::{Decode, DecodeWithMemTracking, Encode};
use safe_math::FixedExt;
use scale_info::TypeInfo;
use sp_std::ops::Neg;
use substrate_fixed::transcendental::exp;
use substrate_fixed::types::{I64F64, U64F64};

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

/// Change produced by rolling a lock forward. Locked mass only ever
/// decreases, but conviction can move either way (it matures upward from
/// locked mass and decays downward once the mass is gone), so its change is
/// carried as separate unsigned growth/decay components.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RollDelta {
    pub locked_mass_delta: AlphaBalance,
    pub conviction_decay: U64F64,
    pub conviction_growth: U64F64,
}

impl RollDelta {
    pub fn zero() -> Self {
        Self {
            locked_mass_delta: AlphaBalance::ZERO,
            conviction_decay: U64F64::saturating_from_num(0),
            conviction_growth: U64F64::saturating_from_num(0),
        }
    }

    pub fn is_zero(&self) -> bool {
        self.locked_mass_delta.is_zero()
            && self.conviction_decay == U64F64::saturating_from_num(0)
            && self.conviction_growth == U64F64::saturating_from_num(0)
    }
}

/// In-memory conviction lock: individual coldkey state plus four aggregate buckets
/// (owner/general × perpetual/decaying), with roll/add/reduce primitives.
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
            roll_delta.conviction_decay,
        );
        // Conviction matured by the individual lock must be credited to the
        // aggregate here: bumping last_update below means the aggregate's own
        // roll-forward will never cover this window, so dropping the growth
        // (as a saturating decrease-only delta used to) permanently
        // understates aggregate conviction.
        aggregate.conviction = aggregate
            .conviction
            .saturating_add(roll_delta.conviction_growth);
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
            conviction_decay: previous_conviction.saturating_sub(rolled.conviction),
            conviction_growth: rolled.conviction.saturating_sub(previous_conviction),
        };

        (rolled, roll_delta)
    }
}
