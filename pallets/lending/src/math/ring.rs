//! Price-ring statistics (SPEC P6, P13, I25): the TWAP median and the mean of a rank ring.

use super::fixed::Price;

/// Lower median of the TWAP samples (SPEC P6; for an even count, the lower middle sample).
/// Sorts `samples` in place. `None` for an empty ring.
#[must_use]
pub fn median_lower(samples: &mut [Price]) -> Option<Price> {
    samples.sort_unstable();
    samples
        .get(samples.len().checked_sub(1)?.checked_div(2)?)
        .copied()
}

/// `floor(sum / count)` of a ring of prices (SPEC P13: the SMA of the daily means, and each
/// daily mean of hourly samples). `None` for an empty ring. Never overflows: quotients and
/// remainders are summed separately.
#[must_use]
pub fn mean(prices: &[Price]) -> Option<Price> {
    let count = u128::try_from(prices.len()).ok()?;
    let mut quotients = 0u128;
    let mut remainders = 0u128;
    for bits in prices.iter().map(|p| p.to_bits()) {
        quotients = quotients.saturating_add(bits.checked_div(count)?);
        remainders = remainders.saturating_add(bits.checked_rem(count)?);
    }
    Some(Price::from_bits(
        quotients.saturating_add(remainders.checked_div(count)?),
    ))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::arithmetic_side_effects)]
    use super::*;

    fn prices(bits: &[u128]) -> Vec<Price> {
        bits.iter().copied().map(Price::from_bits).collect()
    }

    #[test]
    fn median_is_the_lower_middle_sample() {
        assert_eq!(median_lower(&mut []), None);
        assert_eq!(median_lower(&mut prices(&[7])), Some(Price::from_bits(7)));
        assert_eq!(
            median_lower(&mut prices(&[9, 1, 5])),
            Some(Price::from_bits(5))
        );
        assert_eq!(
            median_lower(&mut prices(&[9, 1, 5, 7])),
            Some(Price::from_bits(5))
        );
    }

    #[test]
    fn median_ignores_a_minority_of_outliers() {
        let mut ring = prices(&[100; 72]);
        for sample in ring.iter_mut().take(35) {
            *sample = Price::max_value();
        }
        assert_eq!(median_lower(&mut ring), Some(Price::from_bits(100)));
    }

    #[test]
    fn mean_is_the_floor_of_the_exact_average() {
        assert_eq!(mean(&[]), None);
        assert_eq!(mean(&prices(&[1, 2, 4])), Some(Price::from_bits(2)));
        assert_eq!(mean(&prices(&[5, 5, 5, 6])), Some(Price::from_bits(5)));
        assert_eq!(mean(&prices(&[u128::MAX; 30])), Some(Price::max_value()));
        assert_eq!(
            mean(&prices(&[u128::MAX, u128::MAX - 1, u128::MAX - 3])),
            Some(Price::from_bits(u128::MAX - 2))
        );
    }
}
