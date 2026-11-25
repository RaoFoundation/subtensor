//! Arithmetic that the shared path uses in place of a pass over every UID.

use super::*;

#[test]
fn apportion_equal_units_matches_apportion_units() {
    let check = |count: usize, budget: u64| {
        assert_eq!(
            apportion_equal_units(count, budget),
            apportion_units(&vec![1u128; count], budget),
            "count {count}, budget {budget}"
        );
    };
    // Every small case.
    for count in 0..=70usize {
        for budget in 0..=300u64 {
            check(count, budget);
        }
    }
    let mut rng = Rng(11);
    for count in [
        1usize, 2, 3, 7, 16, 255, 256, 257, 1_000, 2_499, 2_500, 2_501, 4_096, 65_535, 65_536,
    ] {
        let count_units = count as u64;
        for budget in [
            0,
            1,
            count_units - 1,
            count_units,
            count_units + 1,
            2 * count_units + 3,
            1_000_003,
            u64::MAX / 2,
            u64::MAX - count_units,
            u64::MAX - 1,
            u64::MAX,
        ] {
            check(count, budget);
        }
        for _ in 0..40 {
            check(count, rng.next() >> rng.below(64));
        }
    }
}

#[test]
fn apportion_units_ignores_participants_without_shares() {
    // The shared path apportions only the positive shares of a row.
    let mut rng = Rng(12);
    for _ in 0..2_000 {
        let len = rng.range(0, 60) as usize;
        let dense: Vec<u128> = (0..len)
            .map(|_| match rng.below(5) {
                0..=1 => 0,
                2 => u128::from(rng.range(1, 65_535)),
                3 => u128::from(rng.next()) << rng.below(64),
                _ => 1,
            })
            .collect();
        let budget = random_budget(&mut rng);
        let positive: Vec<u128> = dense.iter().copied().filter(|share| *share > 0).collect();
        let expected: Vec<AlphaBalance> = apportion_units(&dense, budget)
            .into_iter()
            .zip(&dense)
            .filter(|(_, share)| **share > 0)
            .map(|(payout, _)| payout)
            .collect();
        assert_eq!(apportion_units(&positive, budget), expected);
        // A participant without a share is never paid.
        for (payout, share) in apportion_units(&dense, budget).iter().zip(&dense) {
            assert!(*share > 0 || payout.is_zero());
        }
    }
}

#[test]
fn equal_incentive_matches_normalized_unit_shares() {
    for count in (1..=3_000u16).chain([4_096, 10_000, 65_535]) {
        let mut report = vec![I64F64::from_num(1); usize::from(count)];
        inplace_normalize_64(&mut report);
        let expected: Vec<u16> = vec_fixed64_to_fixed32(report)
            .iter()
            .map(|value| fixed_proportion_to_u16(*value))
            .collect();
        let equal = SubtensorModule::null_equal_incentive(count);
        assert!(expected.iter().all(|value| *value == equal), "{count}");
        assert_eq!(
            equal,
            fixed_proportion_to_u16(fixed64_to_fixed32(
                I64F64::from_num(1).safe_div(I64F64::from_num(count))
            ))
        );
    }
}

#[test]
fn stored_weight_round_trips_through_the_reference_conversion() {
    // The reference converts each weight to fixed point and back before it uses it as a
    // share. Every weight is in range, so that conversion returns it unchanged.
    for weight in 0..=u16::MAX {
        assert_eq!(
            I32F32::from_num(weight).checked_to_num::<u16>(),
            Some(weight)
        );
    }
}
