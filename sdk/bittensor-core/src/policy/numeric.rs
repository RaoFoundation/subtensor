//! Exact decimal arithmetic used at SDK language boundaries.
// BigInt/BigRational arithmetic cannot overflow; scalar exponent bounds are
// checked before allocation and negation.
#![allow(clippy::arithmetic_side_effects)]
use super::PolicyError;
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{One, Signed, Zero};

pub(crate) fn decimal(text: &str, maximum_exponent: i32) -> Result<BigRational, PolicyError> {
    let text = text.trim();
    let (mantissa, exponent) = match text.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => (
            mantissa,
            exponent
                .parse::<i32>()
                .map_err(|_| PolicyError::range_error("decimal exponent out of range"))?,
        ),
        None => (text, 0),
    };
    let negative = mantissa.starts_with('-');
    let unsigned = mantissa.strip_prefix(['-', '+']).unwrap_or(mantissa);
    let (whole, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    if whole.is_empty() && fraction.is_empty()
        || !whole
            .bytes()
            .chain(fraction.bytes())
            .all(|byte| byte.is_ascii_digit())
    {
        return Err(PolicyError::type_error("invalid decimal value"));
    }
    let power = exponent
        .checked_sub(
            i32::try_from(fraction.len())
                .map_err(|_| PolicyError::range_error("decimal exponent out of range"))?,
        )
        .ok_or_else(|| PolicyError::range_error("decimal exponent out of range"))?;
    if power.unsigned_abs() > maximum_exponent as u32 {
        return Err(PolicyError::range_error("decimal exponent out of range"));
    }
    let mut numerator = format!("{whole}{fraction}")
        .parse::<BigInt>()
        .map_err(|_| PolicyError::type_error("invalid decimal value"))?;
    if negative {
        numerator = -numerator;
    }
    let scale = BigInt::from(10u8).pow(power.unsigned_abs());
    Ok(if power < 0 {
        BigRational::new(numerator, scale)
    } else {
        BigRational::from_integer(numerator * scale)
    })
}

pub(crate) fn round_even(value: &BigRational) -> BigInt {
    let negative = value.is_negative();
    let value = value.abs();
    let mut quotient = value.numer() / value.denom();
    let remainder = value.numer() % value.denom();
    if &remainder * 2u8 > *value.denom()
        || &remainder * 2u8 == *value.denom() && !(&quotient % 2u8).is_zero()
    {
        quotient += BigInt::one();
    }
    if negative {
        -quotient
    } else {
        quotient
    }
}
