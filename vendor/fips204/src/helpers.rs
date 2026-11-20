use crate::types::{R, T, T0};
use crate::{Q, ZETA};

// Some arith routines leverage dilithium https://github.com/PQClean/PQClean/tree/master/crypto_sign

// # Algorithm 43 `BitRev8()` is not implemented; zetas are pulled from pre-computed table
// `ZETA_TABLE_MONT`; see below (near end)

/// # Macro ensure!()
/// If the condition is not met, return an error Result. Borrowed from the `anyhow` crate.
macro_rules! ensure {
    ($cond:expr, $msg:literal $(,)?) => {
        if !$cond {
            return Err($msg);
        }
    };
}

pub(crate) use ensure; // make available throughout crate

/// Ensure all coefficients of polynomial `w` are within -lo to +hi (inclusive)
/// Note, while both range parameters are i32, they should be both non-negative
pub(crate) fn is_in_range(w: &R, lo: i32, hi: i32) -> bool {
    w.0.iter().all(|&e| (e >= -lo) && (e <= hi)) // success is CT, failure vartime
}

/// Partial Barrett-style reduction
// Arguably very slightly faster than single-step i128 below; worth more experimentation
#[allow(clippy::cast_possible_truncation)]
pub(crate) const fn partial_reduce64(a: i64) -> i32 {
    const M: i64 = (1 << 48) / (Q as i64);
    debug_assert!(a.abs() < (67_058_539 << 32), "partial_reduce64 input");
    let x = a >> 23;
    let a = a - x * (Q as i64);
    let x = a >> 23;
    let a = a - x * (Q as i64);
    let q = (a * M) >> 48;
    let res = a - q * (Q as i64);
    debug_assert!(res.abs() < 2 * Q as i64, "partial_reduce64 output");
    res as i32
}

#[allow(dead_code, clippy::cast_possible_truncation)] // I may come back to this and experiment more
pub(crate) const fn partial_reduce64b(a: i64) -> i32 {
    const MM: i64 = ((1 << 64) / (Q as i128)) as i64;
    let q = (a as i128 * MM as i128) >> 64; // only top half is relevant
    let res = a - (q as i64 * Q as i64);
    debug_assert!(res.abs() < 2 * Q as i64, "partial_reduce64b output");
    res as i32
}

/// Partially reduce a signed 32-bit value mod Q ---> `-Q <~ result <~ Q`
// Considering the positive case for `a`, bits 23 and above can be loosely
// viewed as the 'number of Q' contained within `a` (with some rounding-down
// error). So, increment these bits and then subtract off the corresponding
// number of Q. The result is within (better than) -Q < res < Q.
pub(crate) const fn partial_reduce32(a: i32) -> i32 {
    debug_assert!(a.abs() < 2_143_289_344, "partial_reduce32 input");
    let x = (a + (1 << 22)) >> 23;
    let res = a - x * Q;
    debug_assert!(res.abs() < Q, "partial_reduce32 output");
    res
}

pub(crate) const fn full_reduce32(a: i32) -> i32 {
    debug_assert!(a.abs() < 2_143_289_344, "full_reduce32 input");
    let x = partial_reduce32(a); // puts us within better than -Q to +Q
    let res = x + ((x >> 31) & Q); // add Q if negative
    debug_assert!(res < Q, "full_reduce32 output");
    res
}

// Note: this is only used on 'fixed' security parameters (not secret values), so as not to impact CT
/// Bit length required to express `a` in bits
pub(crate) const fn bit_length(x: i32) -> usize {
    x.ilog2() as usize + 1
}

/// Mod +/- see definition on page 6.
/// If `α` is a positive integer and `m ∈ Z` or `m ∈ Z_α` , then m mod± α denotes the unique
/// element `m′ ∈ Z` in the range `−α/2 < m′ ≤ α/2` such that `m` and `m′` are congruent
/// modulo `α`.  'ready to optimize'
pub(crate) fn center_mod(m: i32) -> i32 {
    debug_assert!(m.abs() < 2_143_289_344, "center_mod input"); // for clarity; caught in full_reduce32
    let t = full_reduce32(m);
    let over2 = (Q / 2) - t; // check if t is larger than Q/2
    let res = t - ((over2 >> 31) & Q); // sub Q if over2 is negative
    debug_assert_eq!(m.rem_euclid(Q), res.rem_euclid(Q), "center_mod output");
    res
}

/// Matrix by vector multiplication; e.g., fips 203 top of page 10, first row: `w_hat` = `A_hat` mul `u_hat`
#[must_use]
pub(crate) fn mat_vec_mul<const K: usize, const L: usize>(
    a_hat: &[[T; L]; K],
    u_hat: &[T; L],
) -> [T; K] {
    let mut w_hat = [T0; K];
    let u_hat_mont = to_mont(u_hat);
    for i in 0..K {
        #[allow(clippy::needless_range_loop)] // clarity
        for j in 0..L {
            w_hat[i].0.iter_mut().enumerate().for_each(|(n, e)| {
                *e += mont_reduce(i64::from(a_hat[i][j].0[n]) * i64::from(u_hat_mont[j].0[n]));
            });
        }
    }
    w_hat
}

// Note Algorithm 44 has been dissolved into its place of use(s)

/// # Algorithm 46: `AddVectorNTT(v_hat, w_hat)` on page 45.
/// Computes the sum `v_hat + w_hat` of two vectors `v_hat`, `w_hat` over `𝑇_𝑞`.
///
/// **Input**:  `ℓ ∈ ℕ, v_hat ∈ 𝑇_𝑞^ℓ , w_hat ∈ 𝑇_𝑞^ℓ`. <br>
/// **Output**: `u_hat ∈ 𝑇_𝑞^ℓ`.
#[must_use]
pub(crate) fn add_vector_ntt<const K: usize>(v_hat: &[R; K], w_hat: &[R; K]) -> [R; K] {
    core::array::from_fn(|k| R(core::array::from_fn(|n| v_hat[k].0[n] + w_hat[k].0[n])))
}

#[allow(clippy::cast_possible_truncation)] // as i32
pub(crate) fn to_mont<const L: usize>(vec_a: &[T; L]) -> [T; L] {
    core::array::from_fn(|l| {
        T(core::array::from_fn(|n| {
            partial_reduce64(i64::from(vec_a[l].0[n]) << 32)
        }))
    })
}

pub(crate) fn infinity_norm<const ROW: usize>(w: &[R; ROW]) -> i32 {
    w.iter()
        .flat_map(|row| row.0)
        .map(|element| center_mod(element).abs())
        // .max() might be non-CT on some targets. infinity_norm() is used in signature generation and
        // verification; the values are ultimately revealed in the signature, so worst case is leaking
        // which vector element failed. Not a problem since the whole thing is permutation-agnostic
        .max()
        .expect("infinity norm fails")
}

/// # Algorithm 49: MontgomeryReduce(𝑎) on page 50.
/// Computes 𝑎 ⋅ 2−32 mod 𝑞.
///
/// **Input**:  Integer 𝑎 with −2^{31}*𝑞 ≤ 𝑎 ≤ 2^{31}*𝑞.
/// **Output**: 𝑟 ≡ 𝑎 ⋅ 2^{−32} mod 𝑞.
#[allow(clippy::cast_possible_truncation)] // a as i32, res as i32
pub(crate) const fn mont_reduce(a: i64) -> i32 {
    const QINV: i32 = 58_728_449; // (Q * QINV) % 2**32 = 1
    debug_assert!(a >= -17_996_808_479_301_632, "mont_reduce input (a)");
    debug_assert!(a <= 17_996_808_470_921_215, "mont_reduce input (b)");
    let t = (a as i32).wrapping_mul(QINV);
    let res = (a - (t as i64).wrapping_mul(Q as i64)) >> 32;
    debug_assert!(res < (Q as i64), "mont_reduce output 1");
    debug_assert!(-(Q as i64) < res, "mont_reduce output 2");
    res as i32
}

// ----- The following function only runs at compile time (thus, not CT etc) -----

#[allow(clippy::cast_possible_truncation)]
const fn gen_zeta_table_mont() -> [i32; 256] {
    let mut result = [0i32; 256];
    let mut x = 1i64;
    let mut i = 0u32;
    while i < 256 {
        result[(i as u8).reverse_bits() as usize] = ((x << 32) % (Q as i64)) as i32;
        x = (x * ZETA as i64) % (Q as i64);
        i += 1;
    }
    result
}

pub(crate) static ZETA_TABLE_MONT: [i32; 256] = gen_zeta_table_mont();

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_zeta() {
        let val = gen_zeta_table_mont();
        assert_eq!(val[0], 4_193_792);
        assert_eq!(val[1], 25_847);
        assert_eq!(val[2], 5_771_523);
    }

    #[test]
    fn test_partial_reduce64b() {
        // Test with various input values
        assert_eq!(partial_reduce64b(0), 0);
        assert_eq!(
            partial_reduce64b(i64::from(Q)),
            partial_reduce64(i64::from(Q))
        );
        assert_eq!(
            partial_reduce64b(i64::from(-Q)),
            partial_reduce64b(i64::from(-Q))
        );

        // Test with large positive and negative values
        let large_pos = i64::MAX / 64;
        let large_neg = -i64::MAX / 64;
        assert!(partial_reduce64b(large_pos).abs() < 2 * Q);
        assert!(partial_reduce64b(large_neg).abs() < 2 * Q);

        // Test with some specific values
        assert_eq!(partial_reduce64b(12_345_678), partial_reduce64(12_345_678));
        assert_eq!(
            partial_reduce64b(-12_345_678),
            partial_reduce64(-12_345_678)
        );
    }
}
