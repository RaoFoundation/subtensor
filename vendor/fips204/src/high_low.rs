// This file implements functionality from FIPS 204 section 7.4 High-Order and Low-Order Bits and Hints

use crate::helpers::full_reduce32;
use crate::types::{Zq, R};
use crate::{D, Q};

// Some arith routines leverage dilithium https://github.com/PQClean/PQClean/tree/master/crypto_sign

/// # Algorithm 35: `Power2Round(r)` on page 40, but applied over all coefficients of K polys.
/// Decomposes `r` into `(r1, r0)` such that `r ≡ r1·2^d + r0 mod q`.
///
/// **Input**: `r ∈ Z_q`. <br>
/// **Output**: Integers `(r1, r0)`.
pub(crate) fn power2round<const K: usize>(r: &[R; K]) -> ([R; K], [R; K]) {
    // 1: r+ ← r mod q
    // 2: r0 ← r+ mod±2^d
    // 3: return ((r+ − r0)/2^d, r0)

    // Check input ranges
    debug_assert!(
        r.iter()
            .flat_map(|row| row.0)
            .all(|element| (0..Q).contains(&element)),
        "power2round input"
    );

    let r_1: [R; K] = core::array::from_fn(|k| {
        R(core::array::from_fn(|n| {
            (r[k].0[n] + (1 << (D - 1)) - 1) >> D
        }))
    });

    let r_0: [R; K] =
        core::array::from_fn(|k| R(core::array::from_fn(|n| r[k].0[n] - (r_1[k].0[n] << D))));

    // Check output reconstruction
    debug_assert!(
        {
            let mut result = true;
            for k in 0..K {
                for n in 0..256 {
                    result &= r[k].0[n] == ((r_1[k].0[n] << D) + r_0[k].0[n]);
                }
            }
            result
        },
        "Alg 35: fails"
    );

    (r_1, r_0)
}

/// # Algorithm 36: `Decompose(r)` on page 40.
/// Decomposes `r` into `(r1, r0)` such that `r ≡ r1·(2·γ_2) + r0 mod q`.
///
/// If using the 'centered' `r0` yields `r1 = (q - 1)/(2*gamma2)` then the result should
/// be adjusted by setting `r1` to `0`, and subtracting `1` from `r0`. In practice, this
/// happens only with one input value of `rp` for each possible `gamma2`: <br>
/// `        gamma2        rp      r0'   r1'        r0     r1` <br>
/// `         95232   8285185   -95231    44    -95232      0` <br>
/// `        261888   8118529  -261887    16   -261888      0` <br>
/// For each `gamma2` and the specific `r mod q` value, `r0'` is `rp mod± 2*gamma2`, and
/// `r1'` is `rp - 2*gamma2*r0`. Values `(r0,r1)` are the value that just be returned in
/// that case.
///
/// **Input**: `r ∈ Z_q` <br>
/// **Output**: Integers `(r1, r0)`.
pub(crate) fn decompose(gamma2: i32, r: Zq) -> (Zq, Zq) {
    // 1: r+ ← r mod q
    // 2: r0 ← r+ mod±(2γ_2)
    // 3: if r+ − r0 = q − 1 then
    // 4: r1 ← 0
    // 5: r0 ← r0 − 1
    // 6: else r_1 ← (r+ − r0)/(2γ2)
    // 7: end if

    let rp = full_reduce32(r);
    let mut xr1;
    if gamma2 & (1 << 17) == 0 {
        // ml-dsa-44
        xr1 = (rp + 127) >> 7;
        xr1 = (xr1 * 11275 + (1 << 23)) >> 24;
        xr1 ^= ((43 - xr1) >> 31) & xr1;
    } else {
        // ml-dsa-65 and ml-dsa-87
        xr1 = (rp + 127) >> 7;
        xr1 = (xr1 * 1025 + (1 << 21)) >> 22;
        xr1 &= 15;
    }

    let xr0 = rp - xr1 * 2 * gamma2;
    let xr0 = xr0 - ((((Q - 1) / 2 - xr0) >> 31) & Q);

    // Reconstruct/validate outputs
    debug_assert_eq!(
        r.rem_euclid(Q),
        (xr1 * 2 * gamma2 + xr0).rem_euclid(Q),
        "Alg 36: fails"
    );

    (xr1, xr0)
}

/// # Algorithm 37: `HighBits(r)` on page 40.
/// Returns `r1` from the output of `Decompose(r)`.
///
/// **Input**: `r ∈ Z_q` <br>
/// **Output**: Integer `r_1`.
pub(crate) fn high_bits(gamma2: i32, r: Zq) -> Zq {
    //
    // 1: (r1, r0) ← Decompose(r)
    let (r1, _r0) = decompose(gamma2, r);

    // 2: return r1
    r1
}

/// # Algorithm 38: `LowBits(r)` on page 41.
/// Returns `r_0` from the output of Decompose (r).
///
/// **Input**: `r ∈ Z_q` <br>
/// **Output**: Integer `r_0`.
pub(crate) fn low_bits(gamma2: i32, r: Zq) -> Zq {
    //
    // 1: (r1, r0) ← Decompose(r)
    let (_r1, r0) = decompose(gamma2, r);

    // 2: return r0
    r0
}

/// # Algorithm 39: `MakeHint(z,r)` on page 41.
/// Compute hint bit indicating whether adding `z` to `r` alters the high bits of `r`.
///
/// Input: `z`, `r` ∈ `Z_q` <br>
/// Output: Boolean
pub(crate) fn make_hint(gamma2: i32, z: Zq, r: Zq) -> bool {
    //
    // 1: r1 ← HighBits(r)
    let r1 = high_bits(gamma2, r);

    // 2: v1 ← HighBits(r + z)
    let v1 = high_bits(gamma2, r + z);

    // 3: return [[r1 != v1]]
    r1 != v1
}

/// # Algorithm 40: `UseHint(h,r)` on page 41.
/// Returns the high bits of `r` adjusted according to hint `h`.
///
/// This non-CT function uses public data from the signature; thus
/// does not need to be constant time
///
/// **Input**: Boolean `h` (cast in `Z_q`), `r` ∈ `Z_q` <br>
/// **Output**: `r_1 ∈ Z` with `0 ≤ r_1 ≤ (q − 1)/(2·γ_2)`
pub(crate) fn use_hint(gamma2: i32, h: Zq, r: Zq) -> Zq {
    //
    // 1: m ← (q− 1)/(2*γ_2)
    // dissolved into steps 3 and 4 below

    // 2: (r1, r0) ← Decompose(r)
    let (r1, r0) = decompose(gamma2, r);

    // Step 5 here, to simplify later logic
    if h == 0 {
        return r1;
    }

    // 3: if h = 1 and r0 > 0 return (r1 + 1) mod m
    // 4: if h = 1 and r0 ≤ 0 return (r1 − 1) mod m
    if gamma2 & (1 << 17) == 0 {
        // ml-dsa-44; explicit r1 + 1 mod m(44)
        if r0 > 0 {
            if r1 == 43 {
                return 0;
            }
            return r1 + 1;
        } // explicit r1 - 1 mod m(44)
        if r1 == 0 {
            return 43;
        }
        r1 - 1
    } else {
        // ml-dsa-65 and ml-dsa-87; explicit r1 + 1 mod m(16)
        if r0 > 0 {
            return (r1 + 1) & 15;
        } // explicit r1 - 1 mod m(16)
        (r1 - 1) & 15
    }

    // 5: return r1
    // r1 see first 'if' above
}
