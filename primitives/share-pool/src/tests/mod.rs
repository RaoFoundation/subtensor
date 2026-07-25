//! Unit tests for [`SafeFloat`](crate::SafeFloat) and [`SharePool`](crate::SharePool).

use super::*;
use approx::assert_abs_diff_eq;
use std::collections::BTreeMap;
use std::ops::Neg;
use substrate_fixed::types::U64F64;

struct MockSharePoolDataOperations {
    shared_value: u64,
    share: BTreeMap<u16, SafeFloat>,
    denominator: SafeFloat,
}

impl MockSharePoolDataOperations {
    fn new() -> Self {
        MockSharePoolDataOperations {
            shared_value: 0u64,
            share: BTreeMap::new(),
            denominator: SafeFloat::zero(),
        }
    }
}

impl SharePoolDataOperations<u16> for MockSharePoolDataOperations {
    fn get_shared_value(&self) -> u64 {
        self.shared_value
    }

    fn get_share(&self, key: &u16) -> SafeFloat {
        self.share.get(key).cloned().unwrap_or_else(SafeFloat::zero)
    }

    fn try_get_share(&self, key: &u16) -> Result<SafeFloat, ()> {
        match self.share.get(key).cloned() {
            Some(value) => Ok(value),
            None => Err(()),
        }
    }

    fn get_denominator(&self) -> SafeFloat {
        self.denominator.clone()
    }

    fn set_shared_value(&mut self, value: u64) {
        self.shared_value = value;
    }

    fn set_share(&mut self, key: &u16, share: SafeFloat) {
        self.share.insert(*key, share);
    }

    fn set_denominator(&mut self, update: SafeFloat) {
        self.denominator = update;
    }
}

#[test]
fn test_get_value() {
    let mut mock_ops = MockSharePoolDataOperations::new();
    mock_ops.set_denominator(10u64.into());
    mock_ops.set_share(&1_u16, 3u64.into());
    mock_ops.set_share(&2_u16, 7u64.into());
    mock_ops.set_shared_value(100u64.into());
    let share_pool = SharePool::new(mock_ops);
    let result1 = share_pool.get_value(&1);
    let result2 = share_pool.get_value(&2);
    assert_eq!(result1, 30);
    assert_eq!(result2, 70);
}

#[test]
fn test_division_by_zero() {
    let mut mock_ops = MockSharePoolDataOperations::new();
    mock_ops.set_denominator(SafeFloat::zero()); // Zero denominator
    let pool = SharePool::<u16, MockSharePoolDataOperations>::new(mock_ops);

    let value = pool.get_value(&1);
    assert_eq!(value, 0, "Value should be 0 when denominator is zero");
}

#[test]
fn test_max_shared_value() {
    let mut mock_ops = MockSharePoolDataOperations::new();
    mock_ops.set_shared_value(u64::MAX.into());
    mock_ops.set_share(&1, 3u64.into()); // Use a neutral value for share
    mock_ops.set_share(&2, 7u64.into()); // Use a neutral value for share
    mock_ops.set_denominator(10u64.into()); // Neutral value to see max effect
    let pool = SharePool::<u16, MockSharePoolDataOperations>::new(mock_ops);

    let max_value = pool.get_value(&1) + pool.get_value(&2);
    assert!(u64::MAX - max_value <= 5, "Max value should map to u64 MAX");
}

#[test]
fn test_max_share_value() {
    let mut mock_ops = MockSharePoolDataOperations::new();
    mock_ops.set_shared_value(1_000_000_000u64); // Use a neutral value for shared value
    mock_ops.set_share(&1, (u64::MAX / 2).into());
    mock_ops.set_share(&2, (u64::MAX / 2).into());
    mock_ops.set_denominator((u64::MAX).into());
    let pool = SharePool::<u16, MockSharePoolDataOperations>::new(mock_ops);

    let value1 = pool.get_value(&1) as i128;
    let value2 = pool.get_value(&2) as i128;

    assert_abs_diff_eq!(value1 as f64, 500_000_000_f64, epsilon = 1.);
    assert!((value2 - 500_000_000).abs() <= 1);
}

#[test]
fn test_denom_precision() {
    let mock_ops = MockSharePoolDataOperations::new();
    let mut pool = SharePool::<u16, MockSharePoolDataOperations>::new(mock_ops);

    pool.update_value_for_one(&1, 1000);

    let value_tmp = pool.get_value(&1) as i128;
    assert_eq!(value_tmp, 1000);

    pool.update_value_for_one(&1, -990);
    pool.update_value_for_one(&2, 1000);
    pool.update_value_for_one(&2, -990);

    let value1 = pool.get_value(&1) as i128;
    let value2 = pool.get_value(&2) as i128;

    assert_eq!(value1, 10);
    assert_eq!(value2, 10);
}

// cargo test --package share-pool --lib -- tests::test_denom_high_precision --exact --show-output
#[test]
fn test_denom_high_precision() {
    let mock_ops = MockSharePoolDataOperations::new();
    let mut pool = SharePool::<u16, MockSharePoolDataOperations>::new(mock_ops);

    // 50%/50% stakes consisting of 1 rao each
    pool.update_value_for_one(&1, 1);
    pool.update_value_for_one(&2, 1);

    // Huge emission resulting in 1M Alpha
    // Both stakers should have 500k Alpha each
    pool.update_value_for_all(999_999_999_999_998);

    // Everyone unstakes almost everything, leaving 10 rao in the stake
    pool.update_value_for_one(&1, -499_999_999_999_990);
    pool.update_value_for_one(&2, -499_999_999_999_990);

    // Huge emission resulting in 1M Alpha
    // Both stakers should have 500k Alpha each
    pool.update_value_for_all(999_999_999_999_980);

    // Stakers add 1k Alpha each
    pool.update_value_for_one(&1, 1_000_000_000_000);
    pool.update_value_for_one(&2, 1_000_000_000_000);

    let value1 = pool.get_value(&1) as f64;
    let value2 = pool.get_value(&2) as f64;
    assert_abs_diff_eq!(value1, 501_000_000_000_000_f64, epsilon = 1.);
    assert_abs_diff_eq!(value2, 501_000_000_000_000_f64, epsilon = 1.);
}

// cargo test --package share-pool --lib -- tests::test_denom_high_precision_many_small_unstakes --exact --show-output
#[test]
fn test_denom_high_precision_many_small_unstakes() {
    let mock_ops = MockSharePoolDataOperations::new();
    let mut pool = SharePool::<u16, MockSharePoolDataOperations>::new(mock_ops);

    // 50%/50% stakes consisting of 1 rao each
    pool.update_value_for_one(&1, 1);
    pool.update_value_for_one(&2, 1);

    // Huge emission resulting in 1M Alpha
    // Both stakers should have 500k Alpha + 1 rao each
    pool.update_value_for_all(1_000_000_000_000_000);

    // Run X number of small unstake transactions
    let tx_count = 1000;
    let unstake_amount = -500_000_000;
    for _ in 0..tx_count {
        pool.update_value_for_one(&1, unstake_amount);
        pool.update_value_for_one(&2, unstake_amount);
    }

    // Emit 1M - each gets 500k Alpha
    pool.update_value_for_all(1_000_000_000_000_000);

    // Each adds 1k Alpha
    pool.update_value_for_one(&1, 1_000_000_000_000);
    pool.update_value_for_one(&2, 1_000_000_000_000);

    // Result, each should get
    //   (500k+1) + tx_count * unstake_amount + 500k + 1k
    let value1 = pool.get_value(&1) as i128;
    let value2 = pool.get_value(&2) as i128;
    let expected = 1_001_000_000_000_000 + tx_count * unstake_amount;

    assert_abs_diff_eq!(value1 as f64, expected as f64, epsilon = 1.);
    assert_abs_diff_eq!(value2 as f64, expected as f64, epsilon = 1.);
}

#[test]
fn test_update_value_for_one() {
    let mock_ops = MockSharePoolDataOperations::new();
    let mut pool = SharePool::<u16, MockSharePoolDataOperations>::new(mock_ops);

    pool.update_value_for_one(&1, 1000);

    let value = pool.get_value(&1) as i128;
    assert_eq!(value, 1000);
}

#[test]
fn test_update_value_for_all() {
    let mock_ops = MockSharePoolDataOperations::new();
    let mut pool = SharePool::<u16, MockSharePoolDataOperations>::new(mock_ops);

    pool.update_value_for_all(1000);
    assert_eq!(
        pool.state_ops.shared_value,
        U64F64::saturating_from_num(1000)
    );
}

// cargo test --package share-pool --lib -- tests::test_shares_for_value_update --exact --show-output
#[test]
fn test_shares_for_value_update() {
    // Test case (update, shared_value, denominator_mantissa, denominator_exponent)
    [
        (1_i64, 1_u64, 1_u64, 0_i64),
        (1, 1_000_000_000_000_000_000, 1, 0),
        (1, 21_000_000_000_000_000, 1, 5),
        (1, 21_000_000_000_000_000, 1, -1_000_000),
        (1, 21_000_000_000_000_000, 1, -1_000_000_000),
        (1, 21_000_000_000_000_000, 1, -1_000_000_001),
        (1_000, 21_000_000_000_000_000, 1, 5),
        (21_000_000_000_000_000, 21_000_000_000_000_000, 1, 5),
        (21_000_000_000_000_000, 21_000_000_000_000_000, 1, -5),
        (21_000_000_000_000_000, 21_000_000_000_000_000, 1, -100),
        (21_000_000_000_000_000, 21_000_000_000_000_000, 1, 100),
        (210_000_000_000_000_000, 21_000_000_000_000_000, 1, 5),
        (1_000, 1_000, 21_000_000_000_000_000, 0),
        (1_000, 1_000, 21_000_000_000_000_000, -1),
    ]
    .into_iter()
    .for_each(
        |(update, shared_value, denominator_mantissa, denominator_exponent)| {
            let mock_ops = MockSharePoolDataOperations::new();
            let pool = SharePool::<u16, MockSharePoolDataOperations>::new(mock_ops);

            let denominator_float =
                SafeFloat::new(denominator_mantissa as u128, denominator_exponent)
                    .unwrap_or_default();
            let denominator_f64: f64 = denominator_float.clone().into();
            let spu: f64 = pool
                .shares_for_value_update(update, shared_value, &denominator_float)
                .into();
            let expected = update as f64 * denominator_f64 / shared_value as f64;
            let precision = 1000.;
            assert_abs_diff_eq!(expected, spu, epsilon = expected / precision);
        },
    );
}

#[test]
fn test_safefloat_normalize() {
    // Test case: mantissa, exponent, expected mantissa, expected exponent
    [
        (1_u128, 0, 1_000_000_000_000_000_000_000_u128, -21_i64),
        (0, 0, 0, 0),
        (10_u128, 0, 1_000_000_000_000_000_000_000_u128, -20),
        (1_000_u128, 0, 1_000_000_000_000_000_000_000_u128, -18),
        (
            100_000_000_000_000_000_000_u128,
            0,
            1_000_000_000_000_000_000_000_u128,
            -1,
        ),
        (SAFE_FLOAT_MAX, 0, SAFE_FLOAT_MAX, 0),
    ]
    .into_iter()
    .for_each(|(m, e, expected_m, expected_e)| {
        let a = SafeFloat::new(m, e).unwrap();
        assert_eq!(a.mantissa(), expected_m);
        assert_eq!(a.exponent(), expected_e);
    });
}

#[test]
fn test_safefloat_add() {
    // Test case: man_a, exp_a, man_b, exp_b, expected mantissa of a+b, expected exponent of a+b
    [
        // 1 + 1 = 2
        (
            1_u128,
            0,
            1_u128,
            0,
            200_000_000_000_000_000_000_u128,
            -20_i64,
        ),
        // 0 + 1 = 1
        (0, 0, 1, 0, 1_000_000_000_000_000_000_000_u128, -21_i64),
        // 0 + 0.1 = 0.1
        (0, 0, 1, -1, 1_000_000_000_000_000_000_000_u128, -22_i64),
        // 1e-1000 + 0.1 = 0.1
        (1, -1000, 1, -1, 1_000_000_000_000_000_000_000_u128, -22_i64),
        // SAFE_FLOAT_MAX + SAFE_FLOAT_MAX
        (
            SAFE_FLOAT_MAX,
            0,
            SAFE_FLOAT_MAX,
            0,
            SAFE_FLOAT_MAX * 2 / 10,
            1_i64,
        ),
        // Expected loss of precision: tiny + huge
        (
            1_u128,
            0,
            1_000_000_000_000_000_000_000_u128,
            1,
            1_000_000_000_000_000_000_000_u128,
            1_i64,
        ),
        (
            1_u128,
            0,
            1_u128,
            22,
            1_000_000_000_000_000_000_000_u128,
            1_i64,
        ),
        (
            1_u128,
            0,
            1_u128,
            23,
            1_000_000_000_000_000_000_000_u128,
            2_i64,
        ),
        (
            123_u128,
            0,
            1_u128,
            23,
            1_000_000_000_000_000_000_000_u128,
            2_i64,
        ),
        (
            123_u128,
            1,
            1_u128,
            23,
            100_000_000_000_000_000_001_u128,
            3_i64,
        ),
        // Small-ish + very large (10^22 + 42)
        // 42 * 10^0 + 1 * 10^22 ≈ 1e22 + 42
        // Normalized ≈ (1e21 + 4) * 10^1
        (
            42_u128,
            0,
            1_u128,
            22,
            1_000_000_000_000_000_000_000_u128,
            1_i64,
        ),
        // "Almost 10^21" + 10^22
        // (10^21 - 1) + 10^22 → floor((10^22 + 10^21 - 1) / 100) * 10^2
        (
            999_999_999_999_999_999_999_u128,
            0,
            1_u128,
            22,
            109_999_999_999_999_999_999_u128,
            2_i64,
        ),
        // Small-ish + 10^23 where the small part is completely lost
        // 42 + 10^23 -> floor((10^23 + 42)/100) * 10^2 ≈ 1e21 * 10^2
        (
            42_u128,
            0,
            1_u128,
            23,
            1_000_000_000_000_000_000_000_u128,
            2_i64,
        ),
        // Small-ish + 10^23 where tiny part slightly affects mantissa
        // 4200 + 10^23 -> floor((10^23 + 4200)/100) * 10^2 = (1e21 + 42) * 10^2
        (
            4_200_u128,
            0,
            1_u128,
            23,
            100_000_000_000_000_000_004_u128,
            3_i64,
        ),
        // (10^21 - 1) + 10^23
        // -> floor((10^23 + 10^21 - 1)/100) = 1e21 + 1e19 - 1
        (
            999_999_999_999_999_999_999_u128,
            0,
            1_u128,
            23,
            100_999_999_999_999_999_999_u128,
            3_i64,
        ),
        // Medium + 10^23 with exponent 1 on the smaller term
        // 999_999 * 10^1 + 1 * 10^23 -> (10^22 + 999_999) * 10^1
        // Normalized ≈ (1e21 + 99_999) * 10^2
        (
            999_999_u128,
            1,
            1_u128,
            23,
            100_000_000_000_000_009_999_u128,
            3_i64,
        ),
        // Check behaviour with exponent 24, tiny second term
        // 1 * 10^24 + 1 -> floor((10^24 + 1)/1000) * 10^3 ≈ 1e21 * 10^3
        (
            1_u128,
            24,
            1_u128,
            0,
            1_000_000_000_000_000_000_000_u128,
            3_i64,
        ),
        // 1 * 10^24 + a non-trivial small mantissa
        // 1e24 + 123456789012345678901 -> floor(/1000) = 1e21 + 123456789012345678
        (
            1_u128,
            24,
            123_456_789_012_345_678_901_u128,
            0,
            100_012_345_678_901_234_567_u128,
            4_i64,
        ),
        // 10^22 and 10^23 combined:
        // 1 * 10^22 + 1 * 10^23 = 11 * 10^22 = (1.1 * 10^23)
        // Normalized → (1.1e20) * 10^3
        (
            1_u128,
            22,
            1_u128,
            23,
            110_000_000_000_000_000_000_u128,
            3_i64,
        ),
        // Both operands already aligned at a huge scale:
        // (10^21 - 1) * 10^22 + 1 * 10^22 = 10^21 * 10^22 = 10^43
        // Canonical form: (1e21) * 10^22
        (
            999_999_999_999_999_999_999_u128,
            22,
            1_u128,
            22,
            1_000_000_000_000_000_000_000_u128,
            22_i64,
        ),
    ]
    .into_iter()
    .for_each(|(m_a, e_a, m_b, e_b, expected_m, expected_e)| {
        let a = SafeFloat::new(m_a, e_a).unwrap();
        let b = SafeFloat::new(m_b, e_b).unwrap();

        let a_plus_b = a.add(&b).unwrap();
        let b_plus_a = b.add(&a).unwrap();

        assert_eq!(a_plus_b.mantissa(), expected_m);
        assert_eq!(a_plus_b.exponent(), expected_e);
        assert_eq!(b_plus_a.mantissa(), expected_m);
        assert_eq!(b_plus_a.exponent(), expected_e);
    });
}

#[test]
fn test_safefloat_div_by_zero_is_none() {
    let a = SafeFloat::new(1u128, 0).unwrap();
    assert!(a.div(&SafeFloat::zero()).is_none());
}

#[test]
fn test_safefloat_div() {
    // Test case: man_a, exp_a, man_b, exp_b
    [
        (1_u128, 0_i64, 100_000_000_000_000_000_000_u128, -20_i64),
        (1_u128, 0, 1_u128, 0),
        (1_u128, 1, 1_u128, 0),
        (1_u128, 7, 1_u128, 0),
        (1_u128, 50, 1_u128, 0),
        (1_u128, 100, 1_u128, 0),
        (1_u128, 0, 7_u128, 0),
        (1_u128, 1, 7_u128, 0),
        (1_u128, 7, 7_u128, 0),
        (1_u128, 50, 7_u128, 0),
        (1_u128, 100, 7_u128, 0),
        (1_u128, 0, 3_u128, 0),
        (1_u128, 1, 3_u128, 0),
        (1_u128, 7, 3_u128, 0),
        (1_u128, 50, 3_u128, 0),
        (1_u128, 100, 3_u128, 0),
        (2_u128, 0, 3_u128, 0),
        (2_u128, 1, 3_u128, 0),
        (2_u128, 7, 3_u128, 0),
        (2_u128, 50, 3_u128, 0),
        (2_u128, 100, 3_u128, 0),
        (5_u128, 0, 3_u128, 0),
        (5_u128, 1, 3_u128, 0),
        (5_u128, 7, 3_u128, 0),
        (5_u128, 50, 3_u128, 0),
        (5_u128, 100, 3_u128, 0),
        (10_u128, 0, 100_000_000_000_000_000_000_u128, -19),
        (1_000_u128, 0, 100_000_000_000_000_000_000_u128, -17),
        (
            100_000_000_000_000_000_000_u128,
            0,
            1_000_000_000_000_000_000_000_u128,
            -1,
        ),
        (SAFE_FLOAT_MAX, 0, SAFE_FLOAT_MAX, 0),
        (SAFE_FLOAT_MAX, 100, SAFE_FLOAT_MAX, -100),
        (SAFE_FLOAT_MAX, 100, SAFE_FLOAT_MAX - 1, -100),
        (SAFE_FLOAT_MAX - 1, 100, SAFE_FLOAT_MAX, -100),
        (SAFE_FLOAT_MAX - 2, 100, SAFE_FLOAT_MAX, -100),
        (SAFE_FLOAT_MAX, 100, SAFE_FLOAT_MAX / 2 - 1, -100),
        (SAFE_FLOAT_MAX, 100, SAFE_FLOAT_MAX / 2 - 1, 100),
        (1_u128, 0, 100_000_000_000_000_000_000_u128, -20_i64),
        (
            123_456_789_123_456_789_123_u128,
            20_i64,
            87_654_321_987_654_321_987_u128,
            -20_i64,
        ),
        (
            123_456_789_123_456_789_123_u128,
            100_i64,
            87_654_321_987_654_321_987_u128,
            -100_i64,
        ),
        (
            123_456_789_123_456_789_123_u128,
            -100_i64,
            87_654_321_987_654_321_987_u128,
            100_i64,
        ),
        (
            123_456_789_123_456_789_123_u128,
            -99_i64,
            87_654_321_987_654_321_987_u128,
            99_i64,
        ),
        (
            123_456_789_123_456_789_123_u128,
            123_i64,
            87_654_321_987_654_321_987_u128,
            -32_i64,
        ),
        (
            123_456_789_123_456_789_123_u128,
            -123_i64,
            87_654_321_987_654_321_987_u128,
            32_i64,
        ),
    ]
    .into_iter()
    .for_each(|(ma, ea, mb, eb)| {
        let a = SafeFloat::new(ma, ea).unwrap();
        let b = SafeFloat::new(mb, eb).unwrap();

        let actual: f64 = a.div(&b).unwrap().into();
        let expected =
            ma as f64 * (10_f64).powi(ea as i32) / (mb as f64 * (10_f64).powi(eb as i32));

        assert_abs_diff_eq!(actual, expected, epsilon = actual / 100_000_000_000_000_f64);
    });
}

#[test]
fn test_safefloat_mul_div() {
    // result = a * b / c
    // should not lose precision gained in a * b
    // Test case: man_a, exp_a, man_b, exp_b, man_c, exp_c
    [
        (1_u128, -20_i64, 1_u128, -20_i64, 1_u128, -20_i64),
        (123_u128, 20_i64, 123_u128, -20_i64, 321_u128, 0_i64),
        (
            123_123_123_123_123_123_u128,
            20_i64,
            321_321_321_321_321_321_u128,
            -20_i64,
            777_777_777_777_777_777_u128,
            0_i64,
        ),
        (
            11_111_111_111_111_111_111_u128,
            20_i64,
            99_321_321_321_321_321_321_u128,
            -20_i64,
            77_777_777_777_777_777_777_u128,
            0_i64,
        ),
    ]
    .into_iter()
    .for_each(|(ma, ea, mb, eb, mc, ec)| {
        let a = SafeFloat::new(ma, ea).unwrap();
        let b = SafeFloat::new(mb, eb).unwrap();
        let c = SafeFloat::new(mc, ec).unwrap();

        let actual: f64 = a.mul_div(&b, &c).unwrap().into();
        let expected = (ma as f64 * (10_f64).powi(ea as i32))
            * (mb as f64 * (10_f64).powi(eb as i32))
            / (mc as f64 * (10_f64).powi(ec as i32));

        assert_abs_diff_eq!(actual, expected, epsilon = actual / 100_000_000_000_000_f64);
    });
}

#[test]
fn test_safefloat_from_u64f64() {
    [
        // U64F64::from_num(1000.0),
        // U64F64::from_num(10.0),
        // U64F64::from_num(1.0),
        U64F64::from_num(0.1),
        // U64F64::from_num(0.00000001),
        // U64F64::from_num(123_456_789_123_456u128),
        // // Exact zero
        // U64F64::from_num(0.0),
        // // Very small positive value (well above Q64.64 resolution)
        // U64F64::from_num(1e-18),
        // // Value just below 1
        // U64F64::from_num(0.999_999_999_999_999_f64),
        // // Value just above 1
        // U64F64::from_num(1.000_000_000_000_001_f64),
        // // "Random-looking" fractional with many digits
        // U64F64::from_num(1.234_567_890_123_45_f64),
        // // Large integer, but smaller than the max integer part of U64F64
        // U64F64::from_num(999_999_999_999_999_999u128),
        // // Very large integer near the upper bound of integer range
        // U64F64::from_num(u64::MAX as u128),
        // // Large number with fractional part
        // U64F64::from_num(123_456_789_123_456.78_f64),
        // // Medium-large with tiny fractional part to test precision on tail digits
        // U64F64::from_num(1_000_000_000_000.000_001_f64),
        // // Smallish with long fractional part
        // U64F64::from_num(0.123_456_789_012_345_f64),
    ]
    .into_iter()
    .for_each(|f| {
        let safe_float: SafeFloat = f.into();
        let actual: f64 = safe_float.into();
        let expected = f.to_num::<f64>();

        // Relative epsilon ~1e-14 of the magnitude
        let epsilon = if actual == 0.0 {
            0.0
        } else {
            actual.abs() / 100_000_000_000_000_f64
        };

        assert_abs_diff_eq!(actual, expected, epsilon = epsilon);
    });
}

/// This is a real-life scenario test when someone lost 7 TAO on Chutes (SN64)
/// when paying fees in Alpha. The scenario occured because the update of share value
/// of one coldkey (update_value_for_one) hit the scenario of full unstake.
///
/// Specifically, the following condition was triggered:
///
///    `(shared_value + 2_628_000_000_000_000_u64).checked_div(new_denominator)`
///
/// returned None because new_denominator was too low and division of
/// `shared_value + 2_628_000_000_000_000_u64` by new_denominator has overflown U64F64.
///
/// This test fails on the old version of share pool (with much lower tolerances).
///
/// cargo test --package share-pool --lib -- tests::test_loss_due_to_precision --exact --nocapture
#[test]
fn test_loss_due_to_precision() {
    let mock_ops = MockSharePoolDataOperations::new();
    let mut pool = SharePool::<u16, MockSharePoolDataOperations>::new(mock_ops);

    // Setup pool so that initial coldkey's alpha is 10% of 1e12 = 1e11 rao.
    let low_denominator = SafeFloat::new(1u128, -14).unwrap();
    let low_share = SafeFloat::new(1u128, -15).unwrap();
    pool.state_ops.set_denominator(low_denominator);
    pool.state_ops.set_shared_value(1_000_000_000_000_u64);
    pool.state_ops.set_share(&1, low_share);

    let value_before = pool.get_value(&1) as i128;
    assert_abs_diff_eq!(value_before as f64, 100_000_000_000., epsilon = 0.1);

    // Remove a little stake
    let unstake_amount = 1000i64;
    pool.update_value_for_one(&1, unstake_amount.neg());

    let value_after = pool.get_value(&1) as i128;
    assert_abs_diff_eq!(
        (value_before - value_after) as f64,
        unstake_amount as f64,
        epsilon = unstake_amount as f64 / 1_000_000_000.
    );
}

fn rel_err(a: f64, b: f64) -> f64 {
    let denom = a.abs().max(b.abs()).max(1.0);
    (a - b).abs() / denom
}

fn push_unique(v: &mut Vec<u128>, x: u128) {
    if x != 0 && !v.contains(&x) {
        v.push(x);
    }
}

// cargo test --package share-pool --lib -- tests::test_safefloat_mul_div_wide_range --exact --include-ignored --show-output
#[test]
#[ignore = "long-running sweep test; run explicitly when needed"]
fn test_safefloat_mul_div_wide_range() {
    use rayon::prelude::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    // Build mantissa corpus
    let mut mantissas = Vec::<u128>::new();

    let linear_steps: u128 = 200;
    let linear_step = (SAFE_FLOAT_MAX / linear_steps).max(1);
    let mut m = 1u128;
    while m <= SAFE_FLOAT_MAX {
        push_unique(&mut mantissas, m);
        match m.checked_add(linear_step) {
            Some(next) if next > m => m = next,
            _ => break,
        }
    }
    push_unique(&mut mantissas, SAFE_FLOAT_MAX);

    let mut p = 1u128;
    while p <= SAFE_FLOAT_MAX {
        push_unique(&mut mantissas, p);
        if p > 1 {
            push_unique(&mut mantissas, p - 1);
        }
        if let Some(next) = p.checked_add(1)
            && next <= SAFE_FLOAT_MAX
        {
            push_unique(&mut mantissas, next);
        }

        match p.checked_mul(10) {
            Some(next) if next > p && next <= SAFE_FLOAT_MAX => p = next,
            _ => break,
        }
    }

    for delta in [
        0u128, 1, 2, 3, 7, 9, 10, 11, 99, 100, 101, 999, 1_000, 10_000,
    ] {
        if SAFE_FLOAT_MAX > delta {
            push_unique(&mut mantissas, SAFE_FLOAT_MAX - delta);
        }
    }

    mantissas.sort_unstable();
    mantissas.dedup();

    let exp_min: i64 = -120;
    let exp_max: i64 = 120;
    let exp_step: usize = 5;
    let exponents: Vec<i64> = (exp_min..=exp_max).step_by(exp_step).collect();

    // Precompute all (a, b) pairs as outer work items.
    // Each Rayon task will then iterate all c's sequentially.
    let mut outer_cases: Vec<(u128, i64, u128, i64)> = Vec::new();

    for &ma in &mantissas {
        for &ea in &exponents {
            for &mb in &mantissas {
                for &eb in &exponents {
                    outer_cases.push((ma, ea, mb, eb));
                }
            }
        }
    }

    let checked = Arc::new(AtomicUsize::new(0));
    let skipped_non_finite = Arc::new(AtomicUsize::new(0));
    let skipped_invalid_sf = Arc::new(AtomicUsize::new(0));

    let progress_step = 10_000usize;
    let total_outer = outer_cases.len();

    outer_cases.into_par_iter().for_each(|(ma, ea, mb, eb)| {
        let a = match SafeFloat::new(ma, ea) {
            Some(x) => x,
            None => {
                skipped_invalid_sf.fetch_add(1, Ordering::Relaxed);
                return;
            }
        };

        let b = match SafeFloat::new(mb, eb) {
            Some(x) => x,
            None => {
                skipped_invalid_sf.fetch_add(1, Ordering::Relaxed);
                return;
            }
        };

        for &mc in &mantissas {
            for &ec in &exponents {
                let c = match SafeFloat::new(mc, ec) {
                    Some(x) => x,
                    None => {
                        skipped_invalid_sf.fetch_add(1, Ordering::Relaxed);
                        continue;
                    }
                };

                let actual_sf = a.mul_div(&b, &c).unwrap();
                let actual: f64 = actual_sf.into();

                let expected =
                    (ma as f64 * 10_f64.powi(ea as i32))
                    * (mb as f64 * 10_f64.powi(eb as i32))
                    / (mc as f64 * 10_f64.powi(ec as i32));

                if !expected.is_finite() || !actual.is_finite() {
                    skipped_non_finite.fetch_add(1, Ordering::Relaxed);
                    continue;
                }

                let err = rel_err(actual, expected);

                assert!(
                    err <= 1e-12,
                    concat!(
                        "mul_div mismatch:\n",
                        "  a = {}e{}\n",
                        "  b = {}e{}\n",
                        "  c = {}e{}\n",
                        "  actual   = {:.20e}\n",
                        "  expected = {:.20e}\n",
                        "  rel_err  = {:.20e}"
                    ),
                    ma, ea, mb, eb, mc, ec, actual, expected, err
                );

                checked.fetch_add(1, Ordering::Relaxed);
            }
        }

        let done_outer = checked.load(Ordering::Relaxed);
        if done_outer % progress_step == 0 {
            let invalid = skipped_invalid_sf.load(Ordering::Relaxed);
            let non_finite = skipped_non_finite.load(Ordering::Relaxed);
            log::debug!(
                "progress: checked={}, skipped_invalid_sf={}, skipped_non_finite={}, outer_total={}",
                done_outer,
                invalid,
                non_finite,
                total_outer,
            );
        }
    });

    let checked = checked.load(Ordering::Relaxed);
    let skipped_non_finite = skipped_non_finite.load(Ordering::Relaxed);
    let skipped_invalid_sf = skipped_invalid_sf.load(Ordering::Relaxed);

    println!(
        "checked={}, skipped_non_finite={}, skipped_invalid_sf={}, mantissas={}, exponents={}, outer_cases={}",
        checked,
        skipped_non_finite,
        skipped_invalid_sf,
        mantissas.len(),
        exponents.len(),
        total_outer,
    );

    assert!(checked > 0, "test did not validate any finite cases");
}

#[test]
#[ignore = "long-running broad-range test; run explicitly when needed"]
fn test_safefloat_div_wide_range() {
    use rayon::prelude::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn rel_err(a: f64, b: f64) -> f64 {
        let denom = a.abs().max(b.abs()).max(1.0);
        (a - b).abs() / denom
    }

    fn push_unique(v: &mut Vec<u128>, x: u128) {
        if x != 0 && !v.contains(&x) {
            v.push(x);
        }
    }

    // Build a broad mantissa corpus:
    // - coarse linear sweep
    // - powers of 10 and neighbors
    // - values near SAFE_FLOAT_MAX
    let mut mantissas = Vec::<u128>::new();

    let linear_steps: u128 = 200;
    let linear_step = (SAFE_FLOAT_MAX / linear_steps).max(1);
    let mut m = 1u128;
    while m <= SAFE_FLOAT_MAX {
        push_unique(&mut mantissas, m);
        match m.checked_add(linear_step) {
            Some(next) if next > m => m = next,
            _ => break,
        }
    }
    push_unique(&mut mantissas, SAFE_FLOAT_MAX);

    let mut p = 1u128;
    while p <= SAFE_FLOAT_MAX {
        push_unique(&mut mantissas, p);
        if p > 1 {
            push_unique(&mut mantissas, p - 1);
        }
        if let Some(next) = p.checked_add(1)
            && next <= SAFE_FLOAT_MAX
        {
            push_unique(&mut mantissas, next);
        }

        match p.checked_mul(10) {
            Some(next) if next > p && next <= SAFE_FLOAT_MAX => p = next,
            _ => break,
        }
    }

    for delta in [
        0u128, 1, 2, 3, 7, 9, 10, 11, 99, 100, 101, 999, 1_000, 10_000,
    ] {
        if SAFE_FLOAT_MAX > delta {
            push_unique(&mut mantissas, SAFE_FLOAT_MAX - delta);
        }
    }

    mantissas.sort_unstable();
    mantissas.dedup();

    // Exponent sweep.
    // Keep it large enough to stress normalization / exponent math,
    // but still practical for f64 reference calculations.
    let exp_min: i64 = -120;
    let exp_max: i64 = 120;
    let exp_step: usize = 5;
    let exponents: Vec<i64> = (exp_min..=exp_max).step_by(exp_step).collect();

    let m_len = mantissas.len();
    let e_len = exponents.len();
    let total_cases = m_len * e_len * m_len * e_len;

    let checked = Arc::new(AtomicUsize::new(0));
    let skipped_non_finite = Arc::new(AtomicUsize::new(0));
    let skipped_invalid_sf = Arc::new(AtomicUsize::new(0));
    let done_counter = Arc::new(AtomicUsize::new(0));

    (0..total_cases).into_par_iter().for_each(|idx| {
        let mut rem = idx;

        let eb_idx = rem % e_len;
        rem /= e_len;

        let mb_idx = rem % m_len;
        rem /= m_len;

        let ea_idx = rem % e_len;
        rem /= e_len;

        let ma_idx = rem % m_len;

        let ma = mantissas[ma_idx];
        let ea = exponents[ea_idx];
        let mb = mantissas[mb_idx];
        let eb = exponents[eb_idx];

        let a = match SafeFloat::new(ma, ea) {
            Some(x) => x,
            None => {
                skipped_invalid_sf.fetch_add(1, Ordering::Relaxed);
                done_counter.fetch_add(1, Ordering::Relaxed);
                return;
            }
        };

        let b = match SafeFloat::new(mb, eb) {
            Some(x) => x,
            None => {
                skipped_invalid_sf.fetch_add(1, Ordering::Relaxed);
                done_counter.fetch_add(1, Ordering::Relaxed);
                return;
            }
        };

        let actual_sf = match a.div(&b) {
            Some(x) => x,
            None => {
                skipped_invalid_sf.fetch_add(1, Ordering::Relaxed);
                done_counter.fetch_add(1, Ordering::Relaxed);
                return;
            }
        };

        let actual: f64 = actual_sf.into();
        let expected = (ma as f64 * 10_f64.powi(ea as i32)) / (mb as f64 * 10_f64.powi(eb as i32));

        if !actual.is_finite() || !expected.is_finite() {
            skipped_non_finite.fetch_add(1, Ordering::Relaxed);
        } else {
            let err = rel_err(actual, expected);

            assert!(
                err <= 1e-12,
                concat!(
                    "div mismatch:\n",
                    "  a = {}e{}\n",
                    "  b = {}e{}\n",
                    "  actual   = {:.20e}\n",
                    "  expected = {:.20e}\n",
                    "  rel_err  = {:.20e}"
                ),
                ma,
                ea,
                mb,
                eb,
                actual,
                expected,
                err
            );

            checked.fetch_add(1, Ordering::Relaxed);
        }

        let done = done_counter.fetch_add(1, Ordering::Relaxed) + 1;
        if done % 10_000 == 0 {
            let progress = done as f64 / total_cases as f64 * 100.0;
            log::debug!("div progress = {progress:.4}%");
        }
    });

    let checked = checked.load(Ordering::Relaxed);
    let skipped_non_finite = skipped_non_finite.load(Ordering::Relaxed);
    let skipped_invalid_sf = skipped_invalid_sf.load(Ordering::Relaxed);

    println!(
        "div checked={}, skipped_non_finite={}, skipped_invalid_sf={}, mantissas={}, exponents={}, total_cases={}",
        checked,
        skipped_non_finite,
        skipped_invalid_sf,
        mantissas.len(),
        exponents.len(),
        total_cases,
    );

    assert!(checked > 0, "div test did not validate any finite cases");
}
