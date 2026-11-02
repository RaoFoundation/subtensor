//! Per-subnet emission weight vector ("etao").
//!
//! Every subnet carries a five-slot vector of `u64` parts out of
//! [`EMISSION_WEIGHT_ONE`] that sums to exactly one and says how each block's
//! alpha emission is divided:
//!
//! | slot | role      | v1 rule                                   |
//! |-----:|-----------|-------------------------------------------|
//! | 0    | burn      | must be 0 (not yet enabled)               |
//! | 1    | recycle   | must be 0 (not yet enabled)               |
//! | 2    | owner     | `0 ≤ owner ≤ 33%`; leased subnets `≥ 18%` |
//! | 3    | validator | `≤ 50%`, and above root's baseline share  |
//! | 4    | miner     | `≥ 33%`                                   |
//!
//! The full validator slot accrues as one pile. At epoch, root is paid
//! `R_pay = min(P, max(R_nat, a · R_0))` where `P` is the drained validator
//! pile, `R_nat` is root's stake-mix claim on that pile, `R_0 = r · α_out`
//! is the classic baseline (`r = V0 · x_g / (x_g + A)` with
//! `x_g = root_tao · TaoWeight`), and `a ∈ [0, 1]` is root's alpha-dividend
//! yield versus the subnet average (labor-backed floor). Alpha validators
//! receive the remainder. The owner and miner slots are untouched.
//!
//! A set is rejected when the new validator share is at or below the live
//! baseline (`V_new ≤ r`): the unscaled floor would consume the whole slot.
//! If the baseline later grows past `V`, the floor is capped at `P` and
//! alpha validators can still receive nothing until the owner raises `V` or
//! the baseline recedes (subject to `a`).
//!
//! Consensus stake weighting, the root/alpha dividend split and child/parent
//! contributions all use the global `TaoWeight`, exactly as before per-subnet
//! emission weights existed.
use super::*;
use frame_support::{BoundedVec, traits::ConstU32};
use safe_math::*;
use substrate_fixed::types::U96F32;
use subtensor_runtime_common::{AlphaBalance, NetUid};

/// Number of slots in the emission weight vector.
pub const EMISSION_WEIGHT_SLOTS: u32 = 5;

/// Denominator of every emission weight: weights are `u64` parts out of `u64::MAX`.
pub const EMISSION_WEIGHT_ONE: u64 = u64::MAX;

/// Storage shape of a subnet's emission weight vector.
pub type EmissionWeightsVec = BoundedVec<u64, ConstU32<EMISSION_WEIGHT_SLOTS>>;

/// In-memory shape of a subnet's emission weight vector.
pub type EmissionWeights = [u64; EMISSION_WEIGHT_SLOTS as usize];

/// `num / den` expressed in parts of [`EMISSION_WEIGHT_ONE`].
pub const fn emission_fraction(num: u64, den: u64) -> u64 {
    match (EMISSION_WEIGHT_ONE as u128)
        .saturating_mul(num as u128)
        .checked_div(den as u128)
    {
        Some(parts) => parts as u64,
        None => 0,
    }
}

/// Owner slot may be at most 33% of emission.
pub const MAX_OWNER_EMISSION: u64 = emission_fraction(33, 100);
/// Validator slot may be at most 50% of emission.
pub const MAX_VALIDATOR_EMISSION: u64 = emission_fraction(50, 100);
/// Miner slot must be at least 33% of emission.
pub const MIN_MINER_EMISSION: u64 = emission_fraction(33, 100);

/// Distinct storage items [`Pallet::read_subnet_emission_weights_view`] reads:
/// `SubnetEmissionWeights` and `SubnetCreatedViaLease`.
pub const EMISSION_WEIGHTS_VIEW_READS: u64 = 2;

/// A TAO weight in u64 parts of [`EMISSION_WEIGHT_ONE`] as a fraction in `[0, 1]`.
pub fn normalize_tao_weight(parts: u64) -> U96F32 {
    U96F32::saturating_from_num(parts).safe_div(U96F32::saturating_from_num(u64::MAX))
}

/// A share in `[0, 1]` as u64 parts of [`EMISSION_WEIGHT_ONE`].
pub fn emission_share_to_parts(share: U96F32) -> u64 {
    share
        .saturating_mul(U96F32::saturating_from_num(EMISSION_WEIGHT_ONE))
        .saturating_to_num::<u64>()
}

/// Root's classic baseline share of a subnet's emission: `V · x / (x + A)`
/// with `x = root_tao · tao_weight` and `A` the subnet's alpha issuance. With
/// the default validator share and the global `TaoWeight` this is `R_0`'s
/// fraction of `alpha_out` — the labor floor's unscaled ceiling at epoch.
pub fn root_emission_share(
    validator: U96F32,
    tao_weight: U96F32,
    root_tao: U96F32,
    alpha_issuance: U96F32,
) -> U96F32 {
    let weighted_root_tao = root_tao.saturating_mul(tao_weight);
    weighted_root_tao
        .checked_div(weighted_root_tao.saturating_add(alpha_issuance))
        .unwrap_or(U96F32::saturating_from_num(0))
        .saturating_mul(validator)
}

/// Labor-backed yield factor `a = clamp(ȳ_R / ȳ, 0, 1)`.
///
/// Yields are alpha-dividend mass per alpha stake. Pure-root keys (`A = 0`)
/// are excluded from both averages; if the root∩alpha cohort is empty,
/// returns 0 (floor off).
pub fn root_alpha_yield_factor(
    root_cohort_ell: U96F32,
    root_cohort_alpha: U96F32,
    subnet_ell: U96F32,
    subnet_alpha: U96F32,
) -> U96F32 {
    let zero = U96F32::saturating_from_num(0);
    if root_cohort_alpha == zero || subnet_alpha == zero || subnet_ell == zero {
        return zero;
    }
    let y_r = root_cohort_ell.safe_div(root_cohort_alpha);
    let y = subnet_ell.safe_div(subnet_alpha);
    if y == zero {
        return zero;
    }
    y_r.safe_div(y).min(U96F32::saturating_from_num(1))
}

/// Epoch root pay from a unified validator pile:
/// `R_pay = min(P, max(R_nat, a · R_0))`.
pub fn root_pay_with_labor_floor(pile: U96F32, r_nat: U96F32, r0: U96F32, a: U96F32) -> U96F32 {
    let floor = a.saturating_mul(r0);
    let uncapped = if r_nat > floor { r_nat } else { floor };
    if uncapped > pile { pile } else { uncapped }
}

/// Classic baseline `R_0` in alpha for a drained validator pile `P ≈ V · α_out`:
/// `min(P, P · r / V)`.
pub fn root_floor_alpha_for_validator_pile(
    pile: U96F32,
    baseline_r: U96F32,
    validator_v: U96F32,
) -> U96F32 {
    let zero = U96F32::saturating_from_num(0);
    if pile == zero || validator_v == zero {
        return zero;
    }
    let r0 = pile.saturating_mul(baseline_r).safe_div(validator_v);
    if r0 > pile { pile } else { r0 }
}

/// Snapshot of the storage items behind a subnet's emission weight view, each
/// read once. A caller that has paid for [`EMISSION_WEIGHTS_VIEW_READS`]
/// reads owes nothing more.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmissionWeightsView {
    /// `[burn, recycle, owner, validator, miner]`.
    pub weights: EmissionWeights,
    /// Whether the subnet was registered through a lease.
    pub created_via_lease: bool,
}

/// Named slot of the emission weight vector.
#[derive(
    Copy,
    Clone,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Encode,
    Decode,
    DecodeWithMemTracking,
    TypeInfo,
    MaxEncodedLen,
)]
#[repr(u16)]
pub enum EmissionIndex {
    /// Alpha destroyed outright. Reserved: must be zero in v1.
    Burn = 0,
    /// Alpha burned and un-minted from issuance. Reserved: must be zero in v1.
    Recycle = 1,
    /// Subnet owner cut.
    Owner = 2,
    /// Validator dividends (root alpha is carved out of this slot).
    Validator = 3,
    /// Miner incentive (split across mechanisms by `MechanismEmissionSplit`).
    Miner = 4,
}

impl EmissionIndex {
    /// Every slot, in vector order.
    pub const ALL: [EmissionIndex; EMISSION_WEIGHT_SLOTS as usize] = [
        EmissionIndex::Burn,
        EmissionIndex::Recycle,
        EmissionIndex::Owner,
        EmissionIndex::Validator,
        EmissionIndex::Miner,
    ];

    /// Decode a wire index; `None` for reserved / unknown slots.
    pub fn from_u16(value: u16) -> Option<Self> {
        match value {
            0 => Some(Self::Burn),
            1 => Some(Self::Recycle),
            2 => Some(Self::Owner),
            3 => Some(Self::Validator),
            4 => Some(Self::Miner),
            _ => None,
        }
    }

    /// Position of this slot in the vector.
    pub const fn position(self) -> usize {
        self as u16 as usize
    }
}

impl<T: Config> Pallet<T> {
    // ---------------------------------------------------------------------
    // Defaults
    // ---------------------------------------------------------------------

    /// Scale a legacy `u16 / u16::MAX` fraction to `u64 / u64::MAX` parts.
    pub fn scale_u16_fraction_to_emission_weight(value: u16) -> u64 {
        (value as u128)
            .saturating_mul(EMISSION_WEIGHT_ONE as u128)
            .safe_div(u16::MAX as u128) as u64
    }

    /// Canonical owner share for subnets that never set a vector: the legacy
    /// `InitialSubnetOwnerCut` (18% on mainnet) scaled to u64 parts.
    pub fn default_owner_emission() -> u64 {
        Self::scale_u16_fraction_to_emission_weight(T::InitialSubnetOwnerCut::get())
    }

    /// Canonical validator share: half of what remains after the owner cut
    /// (41% on mainnet), matching the pre-etao 50/50 split.
    pub fn default_validator_emission() -> u64 {
        EMISSION_WEIGHT_ONE.saturating_sub(Self::default_owner_emission()) / 2
    }

    /// Lowest owner share a lease-created subnet may set: the canonical
    /// default, so lease dividends can never be squeezed below launch terms.
    pub fn leased_subnet_min_owner_emission() -> u64 {
        Self::default_owner_emission()
    }

    /// Build the canonical vector `[0, 0, owner, validator, miner]`; the miner
    /// slot absorbs any rounding so the vector sums to exactly one.
    pub fn default_emission_weights() -> EmissionWeights {
        let owner = Self::default_owner_emission();
        let validator = Self::default_validator_emission();
        let miner = EMISSION_WEIGHT_ONE
            .saturating_sub(owner)
            .saturating_sub(validator);
        [0, 0, owner, validator, miner]
    }

    /// `[0, 0, owner, validator, miner]` with the non-owner remainder split
    /// evenly between validators and miners (miner absorbs rounding).
    pub fn emission_weights_with_owner(owner: u64) -> EmissionWeights {
        let validator = EMISSION_WEIGHT_ONE.saturating_sub(owner) / 2;
        let miner = EMISSION_WEIGHT_ONE
            .saturating_sub(owner)
            .saturating_sub(validator);
        [0, 0, owner, validator, miner]
    }

    pub(crate) fn emission_weights_to_vec(weights: EmissionWeights) -> EmissionWeightsVec {
        EmissionWeightsVec::truncate_from(weights.to_vec())
    }

    // ---------------------------------------------------------------------
    // Reads
    // ---------------------------------------------------------------------

    /// The subnet's full emission weight vector.
    pub fn get_subnet_emission_weights(netuid: NetUid) -> EmissionWeights {
        let stored = SubnetEmissionWeights::<T>::get(netuid);
        let mut weights = [0u64; EMISSION_WEIGHT_SLOTS as usize];
        for (slot, value) in weights.iter_mut().zip(stored.iter()) {
            *slot = *value;
        }
        weights
    }

    /// One slot of the subnet's emission weight vector.
    pub fn get_subnet_emission_weight(netuid: NetUid, index: EmissionIndex) -> u64 {
        Self::get_subnet_emission_weights(netuid)
            .get(index.position())
            .copied()
            .unwrap_or_default()
    }

    /// Burn share (slot 0). Always zero in v1.
    pub fn get_subnet_burn_emission(netuid: NetUid) -> u64 {
        Self::get_subnet_emission_weight(netuid, EmissionIndex::Burn)
    }

    /// Recycle share (slot 1). Always zero in v1.
    pub fn get_subnet_recycle_emission(netuid: NetUid) -> u64 {
        Self::get_subnet_emission_weight(netuid, EmissionIndex::Recycle)
    }

    /// Owner cut (slot 2) in u64 parts.
    pub fn get_subnet_owner_emission_cut(netuid: NetUid) -> u64 {
        Self::get_subnet_emission_weight(netuid, EmissionIndex::Owner)
    }

    /// Validator share (slot 3) in u64 parts.
    pub fn get_subnet_validator_emission_cut(netuid: NetUid) -> u64 {
        Self::get_subnet_emission_weight(netuid, EmissionIndex::Validator)
    }

    /// Miner share (slot 4) in u64 parts.
    pub fn get_subnet_miner_emission_cut(netuid: NetUid) -> u64 {
        Self::get_subnet_emission_weight(netuid, EmissionIndex::Miner)
    }

    /// Convert u64 parts to a `[0, 1]` fixed-point fraction.
    pub fn emission_weight_to_fixed(weight: u64) -> U96F32 {
        U96F32::saturating_from_num(weight)
            .safe_div(U96F32::saturating_from_num(EMISSION_WEIGHT_ONE))
    }

    /// Owner cut as a `[0, 1]` fraction.
    pub fn get_float_subnet_owner_emission_cut(netuid: NetUid) -> U96F32 {
        Self::emission_weight_to_fixed(Self::get_subnet_owner_emission_cut(netuid))
    }

    /// Validator share as a `[0, 1]` fraction.
    pub fn get_float_subnet_validator_emission_cut(netuid: NetUid) -> U96F32 {
        Self::emission_weight_to_fixed(Self::get_subnet_validator_emission_cut(netuid))
    }

    /// Miner share as a `[0, 1]` fraction.
    pub fn get_float_subnet_miner_emission_cut(netuid: NetUid) -> U96F32 {
        Self::emission_weight_to_fixed(Self::get_subnet_miner_emission_cut(netuid))
    }

    /// Whether the owner receives any emission cut (legacy `OwnerCutEnabled`
    /// view: true iff the owner slot is non-zero).
    pub fn get_owner_cut_enabled(netuid: NetUid) -> bool {
        Self::get_subnet_owner_emission_cut(netuid) > 0
    }

    /// Whether the subnet was registered through a lease (persists after the
    /// lease ends).
    pub fn get_subnet_created_via_lease(netuid: NetUid) -> bool {
        SubnetCreatedViaLease::<T>::get(netuid)
    }

    /// Every storage item behind the emission weight view, each read exactly
    /// once. Views that meter storage (the EVM precompile) charge
    /// [`EMISSION_WEIGHTS_VIEW_READS`] reads and derive the rest from this.
    pub fn read_subnet_emission_weights_view(netuid: NetUid) -> EmissionWeightsView {
        EmissionWeightsView {
            weights: Self::get_subnet_emission_weights(netuid),
            created_via_lease: Self::get_subnet_created_via_lease(netuid),
        }
    }

    /// Root's *baseline* share `r` of `netuid`'s emission at this block: what
    /// root earns with the default validator share `V0`
    /// ([`Self::default_validator_emission`]) and the global `TaoWeight`,
    /// `V0 · x_g / (x_g + A)` with `x_g = root_tao · TaoWeight` and `A` the
    /// subnet's alpha issuance. Recomputed from live chain state on every read;
    /// the subnet's own vector plays no part, so it is exactly what a subnet on
    /// the default split pays root.
    pub fn baseline_root_emission_share(netuid: NetUid) -> U96F32 {
        root_emission_share(
            Self::emission_weight_to_fixed(Self::default_validator_emission()),
            Self::get_tao_weight(),
            U96F32::saturating_from_num(Self::get_subnet_tao(NetUid::ROOT)),
            U96F32::saturating_from_num(Self::get_alpha_issuance(netuid)),
        )
    }

    /// Classic floor ceiling for `netuid`: baseline capped at the validator
    /// slot, `min(V, r)`. Used as unscaled `R_0` (before the labor yield
    /// factor `a`) and for the set-time `ValidatorEmissionTooLowToPreserveRoot`
    /// guard. Effective epoch pay may be lower when `a < 1`.
    pub fn get_root_emission_share(netuid: NetUid) -> U96F32 {
        Self::baseline_root_emission_share(netuid)
            .min(Self::get_float_subnet_validator_emission_cut(netuid))
    }

    /// Labor yield factor `a` from the last epoch that drained this subnet
    /// (`u16::MAX` = 1.0). Defaults to 1.0 before the first drain.
    pub fn get_last_root_alpha_yield_factor(netuid: NetUid) -> U96F32 {
        U96F32::saturating_from_num(LastRootAlphaYieldFactor::<T>::get(netuid))
            .safe_div(U96F32::saturating_from_num(u16::MAX))
    }

    /// Effective labor floor share of emission: `a · min(V, r)`.
    pub fn get_effective_root_floor_share(netuid: NetUid) -> U96F32 {
        Self::get_root_emission_share(netuid)
            .saturating_mul(Self::get_last_root_alpha_yield_factor(netuid))
    }

    /// Whether root's classic baseline exceeds the validator slot (floor
    /// ceiling would consume the whole slot when `a = 1`).
    pub fn is_root_emission_share_capped(netuid: NetUid) -> bool {
        Self::baseline_root_emission_share(netuid)
            > Self::get_float_subnet_validator_emission_cut(netuid)
    }

    // ---------------------------------------------------------------------
    // Writes
    // ---------------------------------------------------------------------

    /// Seed a freshly registered subnet with the canonical vector and reset any
    /// per-subnet state a previous occupant of this netuid may have left
    /// (lease marker, accrued owner cut). The accrued owner cut is written as
    /// an explicit zero so that an *absent* entry always means "pre-dates the
    /// seed migration" (see [`Self::owner_cut_alpha_for_lock_refund`]). Lease
    /// registration sets the marker after this runs.
    pub fn seed_subnet_emission_weights(netuid: NetUid) {
        if netuid.is_root() {
            return;
        }
        let weights = Self::default_emission_weights();
        SubnetEmissionWeights::<T>::insert(netuid, Self::emission_weights_to_vec(weights));
        SubnetCreatedViaLease::<T>::remove(netuid);
        SubnetOwnerCutAccrued::<T>::insert(netuid, AlphaBalance::ZERO);
        LastRootAlphaYieldFactor::<T>::remove(netuid);
    }

    /// The owner α the pre-emission-weights lock refund deducted:
    /// `alpha_issuance × legacy_owner_cut / u16::MAX`, floored. `legacy_owner_cut`
    /// is the frozen global `SubnetOwnerCut` (its setter is deprecated).
    pub fn legacy_owner_cut_accrued(netuid: NetUid, legacy_owner_cut: u16) -> AlphaBalance {
        let issuance: u64 = Self::get_alpha_issuance(netuid).to_u64();
        let fraction = U96F32::saturating_from_num(legacy_owner_cut)
            .safe_div(U96F32::saturating_from_num(u16::MAX));
        U96F32::saturating_from_num(issuance)
            .saturating_mul(fraction)
            .floor()
            .saturating_to_num::<u64>()
            .into()
    }

    /// Owner α to deduct from a legacy lock refund. The accrued history
    /// (`SubnetOwnerCutAccrued`) when the subnet has an entry; otherwise the
    /// pre-upgrade formula. Every subnet registered or migrated after the seed
    /// migration has an entry (possibly zero), so the fallback only fires for a
    /// subnet that was already dissolving when the migration ran and was not
    /// seeded. It can never make the deduction silently zero.
    pub fn owner_cut_alpha_for_lock_refund(netuid: NetUid) -> AlphaBalance {
        match SubnetOwnerCutAccrued::<T>::try_get(netuid) {
            Ok(accrued) => accrued,
            Err(()) => Self::legacy_owner_cut_accrued(netuid, Self::get_subnet_owner_cut()),
        }
    }

    /// Write a vector verbatim: no rule checks, no root-baseline check, no
    /// event. Migration and test plumbing only; extrinsics must go through
    /// [`Self::do_set_subnet_emission_weights`].
    pub fn set_subnet_emission_weights_unchecked(netuid: NetUid, weights: EmissionWeights) {
        SubnetEmissionWeights::<T>::insert(netuid, Self::emission_weights_to_vec(weights));
    }

    /// Apply a sparse `(uids, values)` update on top of `current`. Omitted
    /// slots keep their current value; burn and recycle are always treated as
    /// zero when omitted.
    pub fn merge_emission_weight_updates(
        current: EmissionWeights,
        uids: &[u16],
        values: &[u64],
    ) -> Result<EmissionWeights, Error<T>> {
        ensure!(
            uids.len() == values.len(),
            Error::<T>::EmissionWeightsLengthMismatch
        );
        let mut merged = current;
        for reserved in [EmissionIndex::Burn, EmissionIndex::Recycle] {
            if let Some(target) = merged.get_mut(reserved.position()) {
                *target = 0;
            }
        }
        let mut seen = [false; EMISSION_WEIGHT_SLOTS as usize];
        for (uid, value) in uids.iter().zip(values.iter()) {
            let index =
                EmissionIndex::from_u16(*uid).ok_or(Error::<T>::EmissionWeightIndexOutOfRange)?;
            let slot = index.position();
            ensure!(
                !seen.get(slot).copied().unwrap_or(true),
                Error::<T>::EmissionWeightIndexDuplicated
            );
            if let Some(flag) = seen.get_mut(slot) {
                *flag = true;
            }
            if let Some(target) = merged.get_mut(slot) {
                *target = *value;
            }
        }
        Ok(merged)
    }

    /// Check every v1 rule against a full vector. Each failure names the rule
    /// so callers learn the constraints from the error alone.
    pub fn validate_subnet_emission_weights(
        netuid: NetUid,
        weights: &EmissionWeights,
    ) -> DispatchResult {
        let slot = |index: EmissionIndex| weights.get(index.position()).copied().unwrap_or(0);

        ensure!(
            slot(EmissionIndex::Burn) == 0,
            Error::<T>::BurnEmissionMustBeZero
        );
        ensure!(
            slot(EmissionIndex::Recycle) == 0,
            Error::<T>::RecycleEmissionMustBeZero
        );

        let owner = slot(EmissionIndex::Owner);
        ensure!(
            owner <= MAX_OWNER_EMISSION,
            Error::<T>::OwnerEmissionAboveMaximum
        );
        if Self::get_subnet_created_via_lease(netuid) {
            ensure!(
                owner >= Self::leased_subnet_min_owner_emission(),
                Error::<T>::LeasedSubnetOwnerCutBelowMinimum
            );
        }

        let validator = slot(EmissionIndex::Validator);
        ensure!(
            validator <= MAX_VALIDATOR_EMISSION,
            Error::<T>::ValidatorEmissionAboveMaximum
        );

        ensure!(
            slot(EmissionIndex::Miner) >= MIN_MINER_EMISSION,
            Error::<T>::MinerEmissionBelowMinimum
        );

        let total: u128 = weights.iter().map(|w| *w as u128).sum();
        ensure!(
            total == EMISSION_WEIGHT_ONE as u128,
            Error::<T>::EmissionWeightsDoNotSumToOne
        );
        Ok(())
    }

    /// The validator slot must leave alpha validators something after root is
    /// paid off the top: `V_new > r` where `r` is root's live baseline share
    /// [`Self::baseline_root_emission_share`]. Only `new_validator` and live
    /// chain values go in; the old vector plays no part. Fails with
    /// [`Error::ValidatorEmissionTooLowToPreserveRoot`] when `V_new ≤ r`.
    ///
    /// The comparison is made in u64 parts of [`EMISSION_WEIGHT_ONE`], the
    /// same unit the runtime API reports as `root_baseline_share`, so the
    /// smallest accepted value is exactly `root_baseline_share + 1` and
    /// clients can compute the floor without reproducing fixed-point rounding.
    pub fn ensure_validator_share_above_root_baseline(
        netuid: NetUid,
        new_validator: u64,
    ) -> Result<(), Error<T>> {
        ensure!(
            new_validator > emission_share_to_parts(Self::baseline_root_emission_share(netuid)),
            Error::<T>::ValidatorEmissionTooLowToPreserveRoot
        );
        Ok(())
    }

    /// Merge, validate and store a sparse emission weight update, after
    /// checking the new validator slot clears root's live baseline share, and
    /// emit [`Event::SubnetEmissionWeightsSet`].
    pub fn do_set_subnet_emission_weights(
        netuid: NetUid,
        uids: Vec<u16>,
        values: Vec<u64>,
    ) -> DispatchResult {
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
        ensure!(
            !netuid.is_root(),
            Error::<T>::EmissionWeightsNotSettableOnRoot
        );

        let current = Self::get_subnet_emission_weights(netuid);
        let merged = Self::merge_emission_weight_updates(current, &uids, &values)?;
        Self::validate_subnet_emission_weights(netuid, &merged)?;

        let new_validator = merged
            .get(EmissionIndex::Validator.position())
            .copied()
            .unwrap_or(0);
        Self::ensure_validator_share_above_root_baseline(netuid, new_validator)?;

        SubnetEmissionWeights::<T>::insert(netuid, Self::emission_weights_to_vec(merged));

        Self::deposit_event(Event::SubnetEmissionWeightsSet {
            netuid,
            weights: merged.to_vec(),
        });
        Ok(())
    }
}
