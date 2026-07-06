use codec::{Decode, Encode};
use sp_runtime::RuntimeDebug;

#[subtensor_macros::freeze_struct("36db5a8053b1591e")]
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Encode, Decode, RuntimeDebug)]
pub struct ActiveValidatorStake<AuthorityId> {
    pub authority_id: AuthorityId,
    pub stake: u128,
    pub dkg_x25519_public_key: [u8; 32],
}

#[subtensor_macros::freeze_struct("35cdf9ee2db21525")]
#[derive(Clone, Eq, PartialEq, Encode, Decode, RuntimeDebug)]
pub struct DkgShareAtom<AuthorityId> {
    pub authority_id: AuthorityId,
    pub dkg_x25519_public_key: [u8; 32],
    pub share_id: u32,
    pub weight: u128,
}

#[subtensor_macros::freeze_struct("f2fdb604ecfdbf32")]
#[derive(Clone, Eq, PartialEq, Encode, Decode, RuntimeDebug)]
pub struct DkgAtomPlan<AuthorityId> {
    pub atoms: Vec<DkgShareAtom<AuthorityId>>,
    pub total_weight: u128,
    pub threshold_weight: u128,
}

#[derive(Clone, Eq, PartialEq, RuntimeDebug)]
pub enum DkgWeightingError {
    NoActiveValidators,
    NoEligibleValidators,
    ExcludedStakeTooLarge,
    TooManyAtoms,
    ArithmeticOverflow,
}

fn ceil_div_u128(numerator: u128, denominator: u128) -> Result<u128, DkgWeightingError> {
    if denominator == 0 {
        return Err(DkgWeightingError::ArithmeticOverflow);
    }
    numerator
        .checked_add(denominator.saturating_sub(1))
        .ok_or(DkgWeightingError::ArithmeticOverflow)
        .map(|x| x / denominator)
}

fn threshold_atoms_for_active_stake(
    total_stake: u128,
    eligible_stake: u128,
    total_atoms: u128,
) -> Result<u128, DkgWeightingError> {
    if total_stake == 0 || eligible_stake == 0 || total_atoms == 0 {
        return Err(DkgWeightingError::NoEligibleValidators);
    }
    let numerator = total_stake
        .checked_mul(2)
        .and_then(|x| x.checked_mul(total_atoms))
        .ok_or(DkgWeightingError::ArithmeticOverflow)?;
    let denominator = eligible_stake
        .checked_mul(3)
        .ok_or(DkgWeightingError::ArithmeticOverflow)?;
    let threshold = ceil_div_u128(numerator, denominator)?;
    if threshold == 0 {
        return Err(DkgWeightingError::NoEligibleValidators);
    }
    if threshold > total_atoms {
        return Err(DkgWeightingError::ExcludedStakeTooLarge);
    }
    Ok(threshold)
}

pub fn two_thirds_plus_one(total_weight: u128) -> Result<u128, DkgWeightingError> {
    total_weight
        .checked_mul(2)
        .and_then(|x| x.checked_add(2))
        .map(|x| x / 3)
        .ok_or(DkgWeightingError::ArithmeticOverflow)
}

pub fn plan_stake_weighted_atoms<AuthorityId: Clone + Ord>(
    validators: &[ActiveValidatorStake<AuthorityId>],
    max_atoms: u32,
) -> Result<DkgAtomPlan<AuthorityId>, DkgWeightingError> {
    if max_atoms == 0 {
        return Err(DkgWeightingError::TooManyAtoms);
    }

    let mut active: Vec<_> = validators.iter().filter(|v| v.stake > 0).cloned().collect();
    active.sort_by(|a, b| a.authority_id.cmp(&b.authority_id));
    if active.is_empty() {
        return Err(DkgWeightingError::NoActiveValidators);
    }

    let total_stake = active
        .iter()
        .try_fold(0u128, |acc, v| acc.checked_add(v.stake))
        .ok_or(DkgWeightingError::ArithmeticOverflow)?;
    let total_atoms = u128::from(max_atoms);

    let mut allocation: Vec<(AuthorityId, [u8; 32], u32, u128, u128)> = Vec::new();
    let mut assigned = 0u32;
    let mut eligible_stake = 0u128;
    for validator in &active {
        let scaled = validator
            .stake
            .checked_mul(total_atoms)
            .ok_or(DkgWeightingError::ArithmeticOverflow)?;
        let base_u128 = scaled / total_stake;
        if base_u128 == 0 {
            continue;
        }
        let base: u32 = base_u128
            .try_into()
            .map_err(|_| DkgWeightingError::TooManyAtoms)?;
        let remainder = scaled % total_stake;
        assigned = assigned
            .checked_add(base)
            .ok_or(DkgWeightingError::ArithmeticOverflow)?;
        eligible_stake = eligible_stake
            .checked_add(validator.stake)
            .ok_or(DkgWeightingError::ArithmeticOverflow)?;
        allocation.push((
            validator.authority_id.clone(),
            validator.dkg_x25519_public_key,
            base,
            remainder,
            validator.stake,
        ));
    }

    if allocation.is_empty() {
        return Err(DkgWeightingError::NoEligibleValidators);
    }

    let threshold_weight =
        threshold_atoms_for_active_stake(total_stake, eligible_stake, total_atoms)?;

    let mut extra_order: Vec<usize> = (0..allocation.len()).collect();
    extra_order.sort_by(|a, b| {
        let Some(left) = allocation.get(*a) else {
            return a.cmp(b);
        };
        let Some(right) = allocation.get(*b) else {
            return a.cmp(b);
        };
        right
            .3
            .cmp(&left.3)
            .then_with(|| right.4.cmp(&left.4))
            .then_with(|| left.0.cmp(&right.0))
    });
    for pos in extra_order {
        if assigned >= max_atoms {
            break;
        }
        let Some(allocation_entry) = allocation.get_mut(pos) else {
            return Err(DkgWeightingError::ArithmeticOverflow);
        };
        allocation_entry.2 = allocation_entry
            .2
            .checked_add(1)
            .ok_or(DkgWeightingError::ArithmeticOverflow)?;
        assigned = assigned
            .checked_add(1)
            .ok_or(DkgWeightingError::ArithmeticOverflow)?;
    }
    if assigned != max_atoms {
        return Err(DkgWeightingError::ArithmeticOverflow);
    }

    let atom_capacity =
        usize::try_from(assigned).map_err(|_| DkgWeightingError::ArithmeticOverflow)?;
    let mut atoms = Vec::with_capacity(atom_capacity);
    let mut share_id = 1u32;
    for (authority_id, dkg_x25519_public_key, count, _, _) in allocation {
        for _ in 0..count {
            atoms.push(DkgShareAtom {
                authority_id: authority_id.clone(),
                dkg_x25519_public_key,
                share_id,
                weight: 1,
            });
            share_id = share_id
                .checked_add(1)
                .ok_or(DkgWeightingError::TooManyAtoms)?;
        }
    }

    Ok(DkgAtomPlan {
        atoms,
        total_weight: total_atoms,
        threshold_weight,
    })
}

#[cfg(test)]
mod mev_shield_dkg_weighting_unit_tests {
    use super::*;
    use std::collections::BTreeMap;

    fn validator(id: u8, stake: u128) -> ActiveValidatorStake<Vec<u8>> {
        ActiveValidatorStake {
            authority_id: vec![id],
            stake,
            dkg_x25519_public_key: [id; 32],
        }
    }

    fn atom_counts(plan: &DkgAtomPlan<Vec<u8>>) -> BTreeMap<Vec<u8>, usize> {
        let mut counts = BTreeMap::new();
        for atom in &plan.atoms {
            *counts.entry(atom.authority_id.clone()).or_insert(0) += 1;
        }
        counts
    }

    #[test]
    fn threshold_is_exact_ceil_two_thirds() {
        assert_eq!(two_thirds_plus_one(1).unwrap(), 1);
        assert_eq!(two_thirds_plus_one(2).unwrap(), 2);
        assert_eq!(two_thirds_plus_one(3).unwrap(), 2);
        assert_eq!(two_thirds_plus_one(4).unwrap(), 3);
        assert_eq!(two_thirds_plus_one(10).unwrap(), 7);
        assert_eq!(two_thirds_plus_one(12).unwrap(), 8);
    }

    #[test]
    fn plan_rejects_empty_or_zero_stake_sets() {
        assert_eq!(
            plan_stake_weighted_atoms::<Vec<u8>>(&[], 8).unwrap_err(),
            DkgWeightingError::NoActiveValidators
        );
        assert_eq!(
            plan_stake_weighted_atoms(&[validator(1, 0), validator(2, 0)], 8).unwrap_err(),
            DkgWeightingError::NoActiveValidators
        );
    }

    #[test]
    fn plan_allows_sparse_quantization_without_minimum_one() {
        let validators = vec![validator(1, 1), validator(2, 1), validator(3, 1_000)];
        let plan = plan_stake_weighted_atoms(&validators, 12).unwrap();
        let counts = atom_counts(&plan);
        assert_eq!(counts.get(&vec![1]).copied().unwrap_or_default(), 0);
        assert_eq!(counts.get(&vec![2]).copied().unwrap_or_default(), 0);
        assert_eq!(counts.get(&vec![3]).copied().unwrap_or_default(), 12);
        assert_eq!(plan.total_weight, 12);
        assert_eq!(plan.threshold_weight, 9);
    }

    #[test]
    fn plan_applies_zero_share_cutoff_and_consecutive_share_ids() {
        let plan = plan_stake_weighted_atoms(
            &[validator(1, 100), validator(2, 200), validator(3, 300)],
            12,
        )
        .unwrap();
        assert_eq!(plan.atoms.len(), 12);
        assert_eq!(plan.total_weight, 12);
        assert_eq!(plan.threshold_weight, 8);
        let counts = atom_counts(&plan);
        assert_eq!(counts.get(&vec![1]).copied().unwrap_or_default(), 2);
        assert_eq!(counts.get(&vec![2]).copied().unwrap_or_default(), 4);
        assert_eq!(counts.get(&vec![3]).copied().unwrap_or_default(), 6);
        let ids = plan.atoms.iter().map(|a| a.share_id).collect::<Vec<_>>();
        assert_eq!(ids, (1..=12).collect::<Vec<_>>());
        assert!(plan.atoms.iter().all(|a| a.weight == 1));
    }

    #[test]
    fn plan_ignores_zero_stake_validators_and_preserves_transport_keys() {
        let plan = plan_stake_weighted_atoms(&[validator(9, 0), validator(4, 10)], 4).unwrap();
        assert_eq!(plan.atoms.len(), 4);
        assert!(plan.atoms.iter().all(|a| a.authority_id == vec![4]));
        assert!(
            plan.atoms
                .iter()
                .all(|a| a.dkg_x25519_public_key == [4; 32])
        );
    }

    #[test]
    fn plan_is_deterministic_independent_of_input_order() {
        let a = vec![validator(3, 30), validator(1, 10), validator(2, 20)];
        let b = vec![validator(2, 20), validator(3, 30), validator(1, 10)];
        assert_eq!(
            plan_stake_weighted_atoms(&a, 12),
            plan_stake_weighted_atoms(&b, 12)
        );
    }

    #[test]
    fn plan_detects_stake_overflow() {
        assert_eq!(
            plan_stake_weighted_atoms(&[validator(1, u128::MAX), validator(2, u128::MAX)], 4)
                .unwrap_err(),
            DkgWeightingError::ArithmeticOverflow
        );
    }

    #[test]
    fn plan_rejects_excessive_excluded_stake() {
        let mut validators = vec![validator(1, 60)];
        validators.extend((2u8..=41).map(|id| validator(id, 1)));
        assert_eq!(
            plan_stake_weighted_atoms(&validators, 10).unwrap_err(),
            DkgWeightingError::ExcludedStakeTooLarge
        );
    }
}
