// This file implements functionality from FIPS 204 section 7.5 `NTT` and `invNTT`

use crate::helpers::{full_reduce32, mont_reduce, ZETA_TABLE_MONT};
use crate::types::{R, T};
use crate::Q;

/// # Algorithm 41 NTT(w) on page 43.
/// Computes the Number-Theoretic Transform. An inner loop over `w/w_hat` has
/// been refactored into this function, so it processes an array of elements.
///
/// **Input**: polynomial `w(X) = ∑_{j=0}^{255} w_j X^j ∈ R_q` <br>
/// **Output**: `w_hat = (w_hat[0], ... , w_hat[255]) ∈ T_q`
pub(crate) fn ntt<const KL: usize>(w: &[R; KL]) -> [T; KL] {
    // 1: for j from 0 to 255 do
    // 2: w_hat[j] ← w_j
    // 3: end for
    let mut w_hat: [T; KL] = core::array::from_fn(|x| T(core::array::from_fn(|n| w[x].0[n])));

    // for each element of w_hat
    for w_poly in &mut w_hat {
        //
        // 4: m ← 0
        let mut m = 0;

        // 5: len ← 128
        let mut len = 128;

        // 6: while len ≥ 1 do
        while len >= 1 {
            //
            // 7: start ← 0
            let mut start = 0;

            // 8: while start < 256 do
            while start < 256 {
                //
                // 9: m ← m + 1
                m += 1;

                // 10: zeta ← ζ^{brv(k)} mod q
                let zeta = i64::from(ZETA_TABLE_MONT[m]);

                // 11: for j from start to start + len − 1 do
                for j in start..(start + len) {
                    //
                    // 12: t ← zeta · w_hat[j + len]
                    let t = mont_reduce(zeta * i64::from(w_poly.0[j + len]));

                    // 13: w_hat[j + len] ← w_hat[j] − t
                    w_poly.0[j + len] = w_poly.0[j] - t;

                    // 14: w_hat[j] ← w_hat[j] + t
                    w_poly.0[j] += t;

                    // 15: end for
                }

                // 16: start ← start + 2 · len
                start += 2 * len;

                // 17: end while
            }

            // 18: len ← ⌊len/2⌋
            len >>= 1;

            // 19: end while
        }

        // end for each element of w_hat
    }

    // 20: return ŵ
    w_hat
}

/// # Algorithm 42 NTT−1 (`w_hat`) on page 44.
/// Computes the inverse of the Number-Theoretic Transform. An inner loop over `w/w_hat` has
/// been refactored into this function, so it processes an array of elements.
///
/// **Input**: `w_hat` = `(w_hat[0], . . . , w_hat[255]) ∈ T_q` <br>
/// **Output**: polynomial `w(X) = ∑_{j=0}^{255} w_j X^j ∈ R_q`
pub(crate) fn inv_ntt<const KL: usize>(w_hat: &[T; KL]) -> [R; KL] {
    //
    #[allow(clippy::cast_possible_truncation)]
    const F_MONT: i64 = 8_347_681_i128.wrapping_mul(1 << 32).rem_euclid(Q as i128) as i64;
    //
    // 1: for j from 0 to 255 do
    // 2: w_j ← w_hat[j]
    // 3: end for
    let mut w_out: [R; KL] = core::array::from_fn(|x| R(core::array::from_fn(|n| w_hat[x].0[n])));

    // for each element of w_hat
    for w_poly in &mut w_out {
        //
        // 4: m ← 256
        let mut m = 256;

        // 5: len ← 1
        let mut len = 1;

        // 6: while len < 256 do
        while len < 256 {
            //
            // 7: start ← 0
            let mut start = 0;

            // 8: while start < 256 do
            while start < 256 {
                //
                // 9: m ← m − 1
                m -= 1;

                // 10: zeta ← −ζ^{brv(k)} mod q    ▷ 𝑧 ← −𝜁 BitRev8 (𝑚) mod 𝑞
                let zeta = -ZETA_TABLE_MONT[m];

                // 11: for j from start to start + len − 1 do
                for j in start..(start + len) {
                    //
                    // 12: t ← w_j
                    let t = w_poly.0[j];

                    // 13: w_j ← t + w_{j+len}
                    w_poly.0[j] = t + w_poly.0[j + len];

                    // 14: w_{j+len} ← t − w_{j+len}
                    w_poly.0[j + len] = t - w_poly.0[j + len];

                    // 15: w_{j+len} ← zeta · w_{j+len}
                    w_poly.0[j + len] = mont_reduce(i64::from(zeta) * i64::from(w_poly.0[j + len]));

                    // 16: end for
                }

                // 17: start ← start + 2 · len
                start += 2 * len;

                // 18: end while
            }

            // 19: len ← 2 · len
            len <<= 1;

            // 20: end while
        }

        // 21: f ← 8347681          ▷ f = 256^{−1} mod q
        // 22: for j from 0 to 255 do
        // 23: wj ← f · wj
        for i in &mut w_poly.0 {
            *i = full_reduce32(mont_reduce(F_MONT * i64::from(*i)));
        }

        // 24: end for
    }

    // 25: return w
    w_out
}
