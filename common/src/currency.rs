use core::fmt::{self, Display, Formatter};
use core::ops::{
    Add, AddAssign, BitAnd, BitOr, BitXor, Div, DivAssign, Mul, MulAssign, Not, Rem, RemAssign,
    Shl, Shr, Sub, SubAssign,
};

#[cfg(feature = "approx")]
use approx::AbsDiffEq;
use codec::{
    Compact, CompactAs, Decode, DecodeWithMemTracking, Encode, Error as CodecError, MaxEncodedLen,
};
use frame_support::pallet_prelude::*;
use scale_info::TypeInfo;
use serde::{Deserialize, Serialize};
use substrate_fixed::traits::{Fixed, ToFixed};
use subtensor_macros::freeze_struct;

pub use num_traits::{
    CheckedShl, CheckedShr, FromPrimitive, Num, NumCast, NumOps, PrimInt, Saturating, Signed,
    ToPrimitive, Unsigned, checked_pow,
};
use sp_arithmetic::per_things::Rounding;
use sp_arithmetic::rational::MultiplyRational;
use sp_arithmetic::traits::{
    Bounded, CheckedAdd, CheckedDiv, CheckedMul, CheckedNeg, CheckedRem, CheckedSub, One, Zero,
};

#[cfg(feature = "std")]
use sp_rpc::number::NumberOrHex;

#[freeze_struct("47c5697352027374")]
#[repr(transparent)]
#[derive(
    Deserialize,
    Serialize,
    Clone,
    Copy,
    Decode,
    DecodeWithMemTracking,
    Default,
    Encode,
    Eq,
    Hash,
    MaxEncodedLen,
    Ord,
    PartialEq,
    PartialOrd,
    Debug,
    TypeInfo,
)]
pub struct AlphaBalance(u64);

#[freeze_struct("8d42edeeaa467687")]
#[repr(transparent)]
#[derive(
    Deserialize,
    Serialize,
    Clone,
    Copy,
    Decode,
    DecodeWithMemTracking,
    Default,
    Encode,
    Eq,
    Hash,
    MaxEncodedLen,
    Ord,
    PartialEq,
    PartialOrd,
    Debug,
    TypeInfo,
)]
pub struct TaoBalance(u64);

// implements traits required by the Currency trait (ToFixed + Into<u64> + From<u64>) and CompactAs
// and Display. It expects a wrapper structure for u64 (CurrencyT(u64)).
// TypeInfo is derived on the structs themselves so the type identity (path) is preserved in the
// runtime metadata, letting SDKs generate distinct TaoBalance/AlphaBalance types instead of bare u64.
macro_rules! impl_currency_reqs {
    ($currency_type:ident) => {
        impl $currency_type {
            pub const fn new(inner: u64) -> Self {
                Self(inner)
            }
        }

        impl Display for $currency_type {
            fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
                Display::fmt(&self.0, f)
            }
        }

        impl CompactAs for $currency_type {
            type As = u64;

            fn encode_as(&self) -> &Self::As {
                &self.0
            }

            fn decode_from(v: Self::As) -> Result<Self, CodecError> {
                Ok(Self(v))
            }
        }

        impl From<Compact<$currency_type>> for $currency_type {
            fn from(c: Compact<$currency_type>) -> Self {
                c.0
            }
        }

        impl From<$currency_type> for u64 {
            fn from(val: $currency_type) -> Self {
                val.0
            }
        }

        impl From<u64> for $currency_type {
            fn from(value: u64) -> Self {
                Self(value)
            }
        }

        impl From<u32> for $currency_type {
            fn from(value: u32) -> Self {
                Self(value as u64)
            }
        }

        // TODO: This is a lossy conversion, maybe it should use try_from
        impl From<i32> for $currency_type {
            fn from(value: i32) -> Self {
                Self(value.unsigned_abs().into())
            }
        }

        impl From<u8> for $currency_type {
            fn from(value: u8) -> Self {
                Self(value as u64)
            }
        }

        impl ToFixed for $currency_type {
            fn to_fixed<F: Fixed>(self) -> F {
                self.0.to_fixed()
            }

            fn checked_to_fixed<F: Fixed>(self) -> Option<F> {
                self.0.checked_to_fixed()
            }

            fn saturating_to_fixed<F: Fixed>(self) -> F {
                self.0.saturating_to_fixed()
            }
            fn wrapping_to_fixed<F: Fixed>(self) -> F {
                self.0.wrapping_to_fixed()
            }

            fn overflowing_to_fixed<F: Fixed>(self) -> (F, bool) {
                self.0.overflowing_to_fixed()
            }
        }

        impl Zero for $currency_type {
            fn zero() -> Self {
                Self(0)
            }
            fn is_zero(&self) -> bool {
                Into::<u64>::into(*self) == 0
            }
        }

        impl One for $currency_type {
            fn one() -> Self {
                Self(1)
            }
        }
    };
}

macro_rules! impl_arithmetic_operators {
    ($currency_type:ident) => {
        impl Add for $currency_type {
            type Output = Self;

            #[allow(clippy::arithmetic_side_effects)]
            fn add(self, rhs: Self) -> Self::Output {
                let lhs_u64: u64 = self.into();
                let rhs_u64: u64 = rhs.into();
                (lhs_u64 + rhs_u64).into()
            }
        }

        impl Sub for $currency_type {
            type Output = Self;

            #[allow(clippy::arithmetic_side_effects)]
            fn sub(self, rhs: Self) -> Self::Output {
                let lhs_u64: u64 = self.into();
                let rhs_u64: u64 = rhs.into();
                (lhs_u64 - rhs_u64).into()
            }
        }

        impl Mul for $currency_type {
            type Output = Self;

            #[allow(clippy::arithmetic_side_effects)]
            fn mul(self, rhs: Self) -> Self::Output {
                let lhs_u64: u64 = self.into();
                let rhs_u64: u64 = rhs.into();
                (lhs_u64 * rhs_u64).into()
            }
        }

        impl Div for $currency_type {
            type Output = Self;

            #[allow(clippy::arithmetic_side_effects)]
            fn div(self, rhs: Self) -> Self::Output {
                let lhs_u64: u64 = self.into();
                let rhs_u64: u64 = rhs.into();
                (lhs_u64 / rhs_u64).into()
            }
        }

        impl Rem for $currency_type {
            type Output = Self;

            #[allow(clippy::arithmetic_side_effects)]
            fn rem(self, rhs: Self) -> Self::Output {
                let lhs_u64: u64 = self.into();
                let rhs_u64: u64 = rhs.into();
                (lhs_u64 % rhs_u64).into()
            }
        }

        impl AddAssign for $currency_type {
            #[allow(clippy::arithmetic_side_effects)]
            fn add_assign(&mut self, rhs: Self) {
                *self = *self + rhs;
            }
        }

        impl SubAssign for $currency_type {
            #[allow(clippy::arithmetic_side_effects)]
            fn sub_assign(&mut self, rhs: Self) {
                *self = *self - rhs;
            }
        }

        impl MulAssign for $currency_type {
            #[allow(clippy::arithmetic_side_effects)]
            fn mul_assign(&mut self, rhs: Self) {
                *self = *self * rhs;
            }
        }

        impl DivAssign for $currency_type {
            #[allow(clippy::arithmetic_side_effects)]
            fn div_assign(&mut self, rhs: Self) {
                *self = *self / rhs;
            }
        }
    };
}

macro_rules! impl_approx {
    ($currency_type:ident) => {
        #[cfg(feature = "approx")]
        impl AbsDiffEq<Self> for $currency_type {
            type Epsilon = Self;

            fn default_epsilon() -> Self::Epsilon {
                u64::default_epsilon().into()
            }

            fn abs_diff_eq(&self, other: &Self, epsilon: Self::Epsilon) -> bool {
                u64::abs_diff_eq(
                    &Into::<u64>::into(*self),
                    &Into::<u64>::into(*other),
                    epsilon.into(),
                )
            }

            fn abs_diff_ne(&self, other: &Self, epsilon: Self::Epsilon) -> bool {
                u64::abs_diff_ne(
                    &Into::<u64>::into(*self),
                    &Into::<u64>::into(*other),
                    epsilon.into(),
                )
            }
        }
    };
}

pub trait Token:
    ToFixed
    + Into<u64>
    + From<u64>
    + Clone
    + Copy
    + Eq
    + NumOps
    + Ord
    + PartialEq
    + PartialOrd
    + Display
    + Zero
    + One
{
    const MAX: Self;
    const ZERO: Self;

    fn to_u64(&self) -> u64 {
        (*self).into()
    }

    fn saturating_add(&self, rhv: Self) -> Self {
        Into::<u64>::into(*self).saturating_add(rhv.into()).into()
    }

    #[allow(clippy::arithmetic_side_effects)]
    fn saturating_div(&self, rhv: Self) -> Self {
        Into::<u64>::into(*self).saturating_div(rhv.into()).into()
    }

    fn saturating_sub(&self, rhv: Self) -> Self {
        Into::<u64>::into(*self).saturating_sub(rhv.into()).into()
    }

    fn saturating_mul(&self, rhv: Self) -> Self {
        Into::<u64>::into(*self).saturating_mul(rhv.into()).into()
    }
}

impl_arithmetic_operators!(AlphaBalance);
impl_approx!(AlphaBalance);
impl_currency_reqs!(AlphaBalance);

impl_arithmetic_operators!(TaoBalance);
impl_approx!(TaoBalance);
impl_currency_reqs!(TaoBalance);

impl Token for AlphaBalance {
    const MAX: Self = Self(u64::MAX);
    const ZERO: Self = Self(0);
}

impl Token for TaoBalance {
    const MAX: Self = Self(u64::MAX);
    const ZERO: Self = Self(0);
}

// // Required explicitly by the bound
// impl From<u32> for TaoBalance {
//     fn from(x: u32) -> Self { Self(x as u64) }
// }

impl Bounded for TaoBalance {
    fn min_value() -> Self {
        Self(u64::MIN)
    }
    fn max_value() -> Self {
        Self(u64::MAX)
    }
}

impl Saturating for TaoBalance {
    fn saturating_add(self, rhs: Self) -> Self {
        Self(self.0.saturating_add(rhs.0))
    }
    fn saturating_sub(self, rhs: Self) -> Self {
        Self(self.0.saturating_sub(rhs.0))
    }
}

//////////////////

impl CheckedNeg for TaoBalance {
    fn checked_neg(&self) -> Option<TaoBalance> {
        Some(*self)
    }
}

impl CheckedRem for TaoBalance {
    fn checked_rem(&self, rhs: &Self) -> Option<Self> {
        let lhs_u64: u64 = (*self).into();
        let rhs_u64: u64 = (*rhs).into();
        lhs_u64.checked_rem(rhs_u64).map(Into::into)
    }
}

impl Shl<u32> for TaoBalance {
    type Output = Self;

    #[allow(clippy::arithmetic_side_effects)]
    fn shl(self, rhs: u32) -> Self::Output {
        let lhs: u64 = self.into();
        // Define semantics: saturate to 0 on oversized shift (instead of panic).
        // Alternatively, you could debug_assert! and return 0 in release.
        let shifted = lhs.checked_shl(rhs).unwrap_or(0);
        shifted.into()
    }
}

impl Shr<u32> for TaoBalance {
    type Output = Self;

    #[allow(clippy::arithmetic_side_effects)]
    fn shr(self, rhs: u32) -> Self::Output {
        let lhs: u64 = self.into();
        let shifted = lhs.checked_shr(rhs).unwrap_or(0);
        shifted.into()
    }
}

impl Shl<usize> for TaoBalance {
    type Output = Self;

    #[allow(clippy::arithmetic_side_effects)]
    fn shl(self, rhs: usize) -> Self::Output {
        let lhs: u64 = self.into();
        // Define semantics: saturate to 0 on oversized shift (instead of panic).
        // Alternatively, you could debug_assert! and return 0 in release.
        let shifted = lhs.checked_shl(rhs as u32).unwrap_or(0);
        shifted.into()
    }
}

impl Shr<usize> for TaoBalance {
    type Output = Self;

    #[allow(clippy::arithmetic_side_effects)]
    fn shr(self, rhs: usize) -> Self::Output {
        let lhs: u64 = self.into();
        let shifted = lhs.checked_shr(rhs as u32).unwrap_or(0);
        shifted.into()
    }
}

impl Not for TaoBalance {
    type Output = Self;
    fn not(self) -> Self {
        Self(!self.0)
    }
}
impl BitAnd for TaoBalance {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }
}
impl BitOr for TaoBalance {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl BitXor for TaoBalance {
    type Output = Self;
    fn bitxor(self, rhs: Self) -> Self {
        Self(self.0 ^ rhs.0)
    }
}

impl CheckedAdd for TaoBalance {
    fn checked_add(&self, rhs: &Self) -> Option<Self> {
        self.0.checked_add(rhs.0).map(Self)
    }
}
impl CheckedSub for TaoBalance {
    fn checked_sub(&self, rhs: &Self) -> Option<Self> {
        self.0.checked_sub(rhs.0).map(Self)
    }
}

impl CheckedMul for TaoBalance {
    fn checked_mul(&self, rhs: &Self) -> Option<Self> {
        self.0.checked_mul(rhs.0).map(Self)
    }
}

impl CheckedDiv for TaoBalance {
    fn checked_div(&self, rhs: &Self) -> Option<Self> {
        self.0.checked_div(rhs.0).map(Self)
    }
}

#[allow(clippy::arithmetic_side_effects)]
impl CheckedShl for TaoBalance {
    fn checked_shl(&self, rhs: u32) -> Option<Self> {
        // Validate first (so we can return None), then use the operator as requested.
        let lhs: u64 = (*self).into();
        lhs.checked_shl(rhs)?;
        Some((*self) << rhs)
    }
}

#[allow(clippy::arithmetic_side_effects)]
impl CheckedShr for TaoBalance {
    fn checked_shr(&self, rhs: u32) -> Option<Self> {
        let lhs: u64 = (*self).into();
        lhs.checked_shr(rhs)?;
        Some((*self) >> rhs)
    }
}

impl RemAssign for TaoBalance {
    #[allow(clippy::arithmetic_side_effects)]
    fn rem_assign(&mut self, rhs: Self) {
        *self = *self % rhs;
    }
}

impl PrimInt for TaoBalance {
    fn count_ones(self) -> u32 {
        self.0.count_ones()
    }
    fn count_zeros(self) -> u32 {
        self.0.count_zeros()
    }
    fn leading_zeros(self) -> u32 {
        self.0.leading_zeros()
    }
    fn trailing_zeros(self) -> u32 {
        self.0.trailing_zeros()
    }
    fn rotate_left(self, n: u32) -> Self {
        Self(self.0.rotate_left(n))
    }
    fn rotate_right(self, n: u32) -> Self {
        Self(self.0.rotate_right(n))
    }
    fn signed_shl(self, n: u32) -> Self {
        // For unsigned integers, num_traits defines signed_shl/shr as normal shifts.
        Self(self.0.wrapping_shl(n))
    }
    fn signed_shr(self, n: u32) -> Self {
        Self(self.0.wrapping_shr(n))
    }
    fn unsigned_shl(self, n: u32) -> Self {
        Self(self.0.wrapping_shl(n))
    }
    fn unsigned_shr(self, n: u32) -> Self {
        Self(self.0.wrapping_shr(n))
    }
    fn swap_bytes(self) -> Self {
        Self(self.0.swap_bytes())
    }
    fn from_be(x: Self) -> Self {
        Self(u64::from_be(x.0))
    }
    fn from_le(x: Self) -> Self {
        Self(u64::from_le(x.0))
    }
    fn to_be(self) -> Self {
        Self(self.0.to_be())
    }
    fn to_le(self) -> Self {
        Self(self.0.to_le())
    }
    fn pow(self, exp: u32) -> Self {
        Self(self.0.pow(exp))
    }
}

impl Unsigned for TaoBalance {}

impl Num for TaoBalance {
    type FromStrRadixErr = <u64 as Num>::FromStrRadixErr;

    fn from_str_radix(s: &str, radix: u32) -> Result<Self, Self::FromStrRadixErr> {
        u64::from_str_radix(s, radix).map(Self)
    }
}

impl NumCast for TaoBalance {
    fn from<T: ToPrimitive>(n: T) -> Option<Self> {
        n.to_u64().map(Self)
    }
}

impl ToPrimitive for TaoBalance {
    fn to_i64(&self) -> Option<i64> {
        self.0.to_i64()
    }
    fn to_u64(&self) -> Option<u64> {
        Some(self.0)
    }
    fn to_u128(&self) -> Option<u128> {
        Some(self.0 as u128)
    }
    fn to_usize(&self) -> Option<usize> {
        self.0.to_usize()
    }
}

impl FromPrimitive for TaoBalance {
    fn from_u64(n: u64) -> Option<Self> {
        Some(Self(n))
    }
    fn from_u128(n: u128) -> Option<Self> {
        if n <= u64::MAX as u128 {
            Some(Self(n as u64))
        } else {
            None
        }
    }
    fn from_usize(n: usize) -> Option<Self> {
        Some(Self(n as u64))
    }
    fn from_i64(n: i64) -> Option<Self> {
        if n >= 0 { Some(Self(n as u64)) } else { None }
    }
}

impl MultiplyRational for TaoBalance {
    fn multiply_rational(self, n: Self, d: Self, r: Rounding) -> Option<Self> {
        let a: u64 = self.into();
        let n: u64 = n.into();
        let d: u64 = d.into();
        a.multiply_rational(n, d, r).map(Into::into)
    }
}

impl From<u16> for TaoBalance {
    fn from(x: u16) -> Self {
        TaoBalance(x as u64)
    }
}
impl From<u128> for TaoBalance {
    fn from(n: u128) -> Self {
        if n <= u64::MAX as u128 {
            Self(n as u64)
        } else {
            Self(u64::MAX)
        }
    }
}
impl From<usize> for TaoBalance {
    fn from(n: usize) -> Self {
        Self(n as u64)
    }
}
impl From<sp_core::U256> for TaoBalance {
    fn from(n: sp_core::U256) -> Self {
        if let Ok(n_u64) = n.try_into() {
            Self(n_u64)
        } else {
            Self(u64::MAX)
        }
    }
}

#[allow(clippy::from_over_into)]
impl Into<usize> for TaoBalance {
    fn into(self) -> usize {
        self.0 as usize
    }
}

#[allow(clippy::from_over_into)]
impl Into<u128> for TaoBalance {
    fn into(self) -> u128 {
        self.0 as u128
    }
}

#[allow(clippy::from_over_into)]
impl Into<u32> for TaoBalance {
    fn into(self) -> u32 {
        self.0 as u32
    }
}

#[allow(clippy::from_over_into)]
impl Into<u16> for TaoBalance {
    fn into(self) -> u16 {
        self.0 as u16
    }
}

#[allow(clippy::from_over_into)]
impl Into<u8> for TaoBalance {
    fn into(self) -> u8 {
        self.0 as u8
    }
}

#[allow(clippy::from_over_into)]
impl Into<sp_core::U256> for TaoBalance {
    fn into(self) -> sp_core::U256 {
        sp_core::U256::from(self.0)
    }
}

#[allow(clippy::from_over_into)]
#[cfg(feature = "std")]
impl Into<NumberOrHex> for TaoBalance {
    fn into(self) -> NumberOrHex {
        NumberOrHex::Number(self.0)
    }
}

pub struct ConstTao<const N: u64>;

impl<const N: u64> Get<TaoBalance> for ConstTao<N> {
    fn get() -> TaoBalance {
        TaoBalance::new(N)
    }
}
