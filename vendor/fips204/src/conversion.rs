// This file implements functionality from FIPS 204 section 7.1 Conversion Between Data Types

use crate::helpers::{bit_length, ensure, is_in_range};
use crate::types::{R, R0};
use crate::Q;

// Algorithm 9: `IntegerToBits(x,a)` on page 28 is not needed because the pack and unpack
// algorithms have been reimplemented at a higher level.

// Algorithm 10: `BitsToInteger(y)` on page 28 is not needed because the pack and unpack
// algorithms have been reimplemented at a higher level.

// Algorithm 11: `IntegerToBytes(x,a) on page 28 is not needed because the standard
// `.to_le_bytes()` function is just called instead.

// Algorithm 12: `BitsToBytes(y)` on page 29 is not needed because the pack and unpack
// algorithms have been reimplemented at a higher level.

// Algorithm 13: `BytesToBits(z)` on page 21 is not needed because the pack and unpack
// algorithms have been reimplemented at a higher level.

/// # Algorithm 14: `CoeffFromThreeBytes(b0,b1,b2)` on page 29.
/// Generates an element of `{0, 1, 2, ... , q − 1} ∪ {⊥}` used in rejection sampling.
///
/// This function is used during keygen and signing, but only operates on the non-secret
/// `rho` value stored in the public key, so need not be constant-time in normal
/// operation. To support constant-time `dudect` measurements through the
/// `dudect_keygen_sign_with_rng()` function exposed when the `dudect` feature
/// is enabled, the CTEST value would be set to `true` to effectively bypass the rejection.
///
/// **Input**:  A byte array of length three, representing bytes `b0`, `b1`, `b2`.<br>
/// **Output**: An integer modulo `q` or `⊥` (returned as an Error).
///
/// # Errors
/// Returns an error `⊥` on input 3 bytes forming values between `Q=0x7F_E0_01`--`0x7F_FF_FF`,
/// and between `0xFF_E0_01`--`0xFF_FF_FF` (latter range due to masking of bit 7 of byte 2)
/// per spec; for rejection sampling.
pub(crate) fn coeff_from_three_bytes<const CTEST: bool>(b: [u8; 3]) -> Result<i32, &'static str> {
    // 1: b2′ ← b2
    // 2: if b2′ > 127 then
    // 3:   b2′ ← b2′ − 128     ▷ Set the top bit of b2′ to zero
    // 4: end if
    let b2p = i32::from(b[2] & 0x7F);
    let b2p = if CTEST { b2p & 0x3F } else { b2p }; // Used only for `dudect` measurements

    // 5: z ← 2^{16}·b2′ + 2^8·b1 + b0
    let z = (b2p << 16) | (i32::from(b[1]) << 8) | i32::from(b[0]);

    // 6: if z < q then return z
    if z < Q {
        Ok(z)

        // 7: else return ⊥
    } else {
        Err("Alg 14: returns ⊥")

        // 8: end if
    }
}

/// # Algorithm 15: `CoeffFromHalfByte(b)` on page 30.
/// Generates an element of `{−η, −η + 1, ... , η} ∪ {⊥}` for n ∈ {2, 4}.
///
/// This function is used during keygen, but only operates on the hash-derived
/// `rho_prime` value that is rejection-sampled/expanded into the internal `s_1` and
/// `s_2`, so need not be constant-time in normal operation. To support constant-time
/// `dudect` measurements through the `dudect_keygen_sign_with_rng()` function exposed
/// when the `dudect` feature is enabled, the CTEST value would be set to `true` to
/// effectively bypass the rejection.
///
/// **Input**:  Integer `b` ∈ {0, 1, ... , 15}.
///             Security parameter `η` (eta) must be either 2 or 4.<br>
/// **Output**: An integer between `−η` and `η`, or `⊥`.
///
/// # Errors
/// Returns an error `⊥` on when eta = 4 and b > 8 for rejection sampling. (panics on b > 15)
pub(crate) fn coeff_from_half_byte<const CTEST: bool>(
    eta: i32,
    b: u8,
) -> Result<i32, &'static str> {
    const M5: i32 = ((1i32 << 24) / 5) + 1;
    debug_assert!((eta == 2) || (eta == 4), "Alg 15: incorrect eta");
    debug_assert!(b < 16, "Alg 15: b out of range"); // Note other cases involving b/eta will fall through to Err()

    let b = i32::from(if CTEST { b & 0x07 } else { b });
    // 1: if η = 2 and b < 15 then return 2 − (b mod 5)    ▷ rejection sampling from {−2, … , 2}
    if (eta == 2) && (b < 15) {
        // note b<15, not b<16
        let quot = (b * M5) >> 24;
        let rem = b - quot * 5;
        Ok(2 - rem)

        // 2: else
    } else {
        //
        // 3: if η = 4 and b < 9 then return 4 − b   ▷ rejection sampling from {−4, … , 4}
        if (eta == 4) && (b < 9) {
            Ok(4 - b)

            // 4: else return ⊥
        } else {
            Err("Alg 15: returns ⊥") // not necessarily an error per se, but rather "try again" (we can have eta==2 && b == 15)

            // 5: end if
        }

        // 6: end if
    }
}

/// # Algorithm 16: `SimpleBitPack(w,b)` on page 30.
/// Encodes a polynomial `w` into a byte string. This function is not exposed to unvalidated input.
///
/// **Input**:  `b ∈ N` and `w ∈ R` such that the coefficients of `w` are all in `[0, b]`.
///             Security parameter `b` must be positive and have a bit length of less than 20.<br>
/// **Output**: A byte string of length `32·bitlen(b)`.
pub(crate) fn simple_bit_pack(w: &R, b: i32, bytes_out: &mut [u8]) {
    debug_assert!((1..1024 * 1024).contains(&b), "Alg 16: b out of range"); // plenty of headroom
    debug_assert!(is_in_range(w, 0, b), "Alg 16: w out of range"); // early detect; repeated within bit_pack
    debug_assert_eq!(
        bytes_out.len(),
        32 * bit_length(b),
        "Alg 16: incorrect size of output bytes"
    );

    // 1: 𝑧 ← ()    ▷ set 𝑧 to the empty bit string
    // 2: for 𝑖 from 0 to 255 do
    // 3:   𝑧 ← 𝑧||IntegerToBits(𝑤𝑖 , bitlen 𝑏)
    // 4: end for
    // 5: return BitsToBytes(𝑧)

    // Delegated to `bit_pack()` with lower range set to zero (identical functionality)
    bit_pack(w, 0, b, bytes_out);
}

/// # Algorithm 17: `BitPack(w,a,b)` on page 30.
/// Encodes a polynomial `w` into a byte string.  This function is not exposed to unvalidated input.
///
/// **Input**:  `a, b ∈ N` and `w ∈ R` such that the coefficients of `w` are all in `[−a, b]`.
///             Security parameter `a` must be non-negative and have a bit length of less than 20.
///             Security parameter `b` must be positive and have a bit length of less than 20.<br>
/// **Output**: A byte string of length `32·bitlen(a + b)`.
pub(crate) fn bit_pack(w: &R, a: i32, b: i32, bytes_out: &mut [u8]) {
    debug_assert!((0..(1024 * 1024)).contains(&a), "Alg 17: a out of range");
    debug_assert!((1..(1024 * 1024)).contains(&b), "Alg 17: b out of range");
    debug_assert!(is_in_range(w, a, b), "Alg 17: w out of range");
    debug_assert_eq!(
        w.0.len() * bit_length(a + b),
        bytes_out.len() * 8,
        "Alg 17: bad output size"
    );

    // Original pseudocode
    // 1: 𝑧 ← ()    ▷ set 𝑧 to the empty bit string
    // 2: for 𝑖 from 0 to 255 do
    // 3:   𝑧 ← 𝑧||IntegerToBits(𝑏 − 𝑤𝑖 , bitlen (𝑎 + 𝑏))
    // 4: end for
    // 5: return BitsToBytes(𝑧)

    let bitlen = bit_length(a + b); // Calculate each element bit length
    let mut temp = 0u32; // To insert new values on the left/MSB and pop output values from the right/LSB
    let mut byte_index = 0; // Current output byte position
    let mut bit_index = 0; // Number of bits accumulated in temp

    // For every coefficient in w... (which is known to be in suitably positive range)
    for coeff in w.0 {
        // if we have a negative `a` bound, subtract from b and shift into empty/upper part of temp
        if a > 0 {
            temp |= b.abs_diff(coeff) << bit_index;
        // Otherwise, we just shift and drop into empty/upper part of temp
        } else {
            temp |= coeff.unsigned_abs() << bit_index;
        }
        // account for the amount of bits we now have in temp
        bit_index += bitlen;
        // while we have at least 'bytes' worth of bits in temp
        while bit_index > 7 {
            // drop a byte into the output
            bytes_out[byte_index] = temp.to_le_bytes()[0];
            temp >>= 8;
            // update indexes
            byte_index += 1;
            bit_index -= 8;
        }
    }
}

/// # Algorithm 18: `SimpleBitUnpack(v,b)` on page 31.
/// Reverses the procedure `SimpleBitPack()`.
///
/// Used in Algorithm 23 `pkDecode()` function which does take untrusted input via
/// deserialization (and `verify_start()`).
///
/// **Input**:  `b ∈ N` and a byte string `v` of length 32·bitlen(b).
///             Security parameter `b` must be positive and have a bit length of less than 20.<br>
/// **Output**: A polynomial `w ∈ R`, with coefficients in `[0, 2^c−1]`, where `c = bitlen(b)`.
///             When `b + 1` is a power of 2, the coefficients are in `[0, b]`.
///
/// # Errors
/// Returns an error on `w` out of range.
pub(crate) fn simple_bit_unpack(v: &[u8], b: i32) -> Result<R, &'static str> {
    debug_assert!((1..(1024 * 1024)).contains(&b), "Alg 18: b out of range");
    debug_assert_eq!(v.len(), 32 * bit_length(b), "Alg 18: bad output size");

    // 1: 𝑐 ← bitlen 𝑏
    // 2: 𝑧 ← BytesToBits(𝑣)
    // 3: for 𝑖 from 0 to 255 do
    // 4:   𝑤𝑖 ← BitsToInteger((𝑧[𝑖𝑐], 𝑧[𝑖𝑐 + 1], … 𝑧[𝑖𝑐 + 𝑐 − 1]), 𝑐)
    // 5: end for
    // 6: return 𝑤

    // Delegated to `bit_unpack()` with lower range set to zero (identical functionality)
    // Note that `w_out` is correctly range checked (via ensure!) in `bit_unpack()`
    let w_out = bit_unpack(v, 0, b).map_err(|_| "Alg 18: w out of range")?;
    Ok(w_out)
}

/// # Algorithm 19: `BitUnpack(v,a,b)` on page 31.
/// Reverses the procedure `BitPack()`.
///
/// **Input**:  `a, b ∈ N` and a byte string `v` of length `32·bitlen(a + b)`.
///             Security parameter `a` must be non-negative and have a bit length of less than 20.
///             Security parameter `b` must be positive and have a bit length of less than 20.<br>
/// **Output**: A polynomial `w ∈ R`, with coefficients in `[b − 2^c + 1, b]`, where `c = bitlen(a + b)`. <br>
///             When `a + b + 1` is a power of 2, the coefficients are in `[−a, b]`.
///
/// # Errors
/// Returns an error on `w` out of range.
pub(crate) fn bit_unpack(v: &[u8], a: i32, b: i32) -> Result<R, &'static str> {
    debug_assert!((0..(1024 * 1024)).contains(&a), "Alg 19: a out of range");
    debug_assert!((1..(1024 * 1024)).contains(&b), "Alg 19: b out of range");
    debug_assert_eq!(v.len(), 32 * bit_length(a + b), "Alg 19: bad output size");

    // Original pseudocode
    // 1: 𝑐 ← bitlen (𝑎 + 𝑏)
    // 2: 𝑧 ← BytesToBits(𝑣)
    // 3: for 𝑖 from 0 to 255 do
    // 4:   𝑤𝑖 ← 𝑏 − BitsToInteger((𝑧[𝑖𝑐], 𝑧[𝑖𝑐 + 1], … 𝑧[𝑖𝑐 + 𝑐 − 1]), 𝑐)
    // 5: end for
    // 6: return 𝑤

    let bitlen = bit_length(a + b).try_into().expect("Alg 19: try_into fail");
    let mut w_out = R([0i32; 256]);
    let mut temp = 0i32;
    let mut r_index = 0;
    let mut bit_index = 0;

    for byte in v {
        temp |= i32::from(*byte) << bit_index;
        bit_index += 8;
        while bit_index >= bitlen {
            let tmask = temp & ((1 << bitlen) - 1);
            // choice fixed by security parameter, so CT
            w_out.0[r_index] = if a == 0 { tmask } else { b - tmask };
            bit_index -= bitlen;
            temp >>= bitlen;
            r_index += 1;
        }
    }

    let bot = i32::abs(b - (1 << bitlen) + 1); // b − 2^c + 1 (as abs)
    ensure!(is_in_range(&w_out, bot, b), "Alg 19: w out of range");
    Ok(w_out)
}

/// # Algorithm 20: `HintBitPack(h)` on page 32.
/// Encodes a polynomial vector `h` with binary coefficients into a byte string.
///
/// This function is used during signing, but only to produce the non-secret output
/// signature, so need not be constant-time in normal operation. To support
/// constant-time `dudect` measurements through the `dudect_keygen_sign_with_rng()`
/// function exposed when the `dudect` feature is enabled, the CTEST value would be
/// set to `true` to effectively bypass some of the loop decisions.
///
/// **Input**:  A polynomial vector `h ∈ R^k_2` such that at most `ω` of the coefficients in `h` are equal to `1`.
///             Security parameters `ω` (omega) and k must sum to be less than 256. <br>
/// **Output**: A byte string `y` of length `ω + k`.
pub(crate) fn hint_bit_pack<const CTEST: bool, const K: usize>(
    omega: i32,
    h: &[R; K],
    y_bytes: &mut [u8],
) {
    let omega_u = usize::try_from(omega).expect("Alg 20: try_from fail");
    debug_assert!(
        (1..256).contains(&(omega_u + K)),
        "Alg 20: omega+K out of range"
    );
    debug_assert_eq!(y_bytes.len(), omega_u + K, "Alg 20: bad output size");
    debug_assert!(h.iter().all(|r| is_in_range(r, 0, 1)), "Alg 20: h not 0/1");
    debug_assert!(
        h.iter()
            .all(|r| r.0.iter().filter(|&e| *e == 1).sum::<i32>() <= omega),
        "Alg 20: too many 1's in h"
    );

    // 1: y ∈ B^{ω+k} ← 0^{ω+k}
    y_bytes.iter_mut().for_each(|e| *e = 0);

    // 2: Index ← 0    ▷ Index for writing the first 𝜔 bytes of 𝑦
    let mut index = 0;

    // 3: for i from 0 to k − 1 do    ▷ look at 𝐡[𝑖]
    for i in 0..K {
        //
        // 4: for j from 0 to 255 do
        for j in 0..256 {
            //
            // 5: if h[i]_j != 0 then
            // CT patch path
            if CTEST && (index > (y_bytes.len() - 1)) {
                continue;
            };
            // CT patch path
            if CTEST || (h[i].0[j] != 0) {
                //
                // 6: y[Index] ← j      ▷ Store the locations of the nonzero coefficients in h[i]
                y_bytes[index] = j.to_le_bytes()[0]; // ...bytes()[0] for clippy benefit

                // 7: Index ← Index + 1
                index += 1;

                // 8: end if
            }

            // 9: end for
        }

        // 10: y[ω + i] ← Index     ▷ after processing 𝐡[𝑖], store the value of Index
        y_bytes[omega_u + i] = index.to_le_bytes()[0];

        // 11: end for
    }

    // 12: return y
}

/// # Algorithm 21: `HintBitUnpack(y)` on page 32.
/// Reverses the procedure `HintBitPack()`.
///
/// **Input**:  A byte string `y` of length `ω + k`.
///             Security parameters `ω` (omega) and k must sum to be less than 256. <br>
/// **Output**: A polynomial vector `h ∈ R^k_2` or `⊥` (as an Error).
///
/// # Errors
/// Returns an error on invalid input.
pub(crate) fn hint_bit_unpack<const K: usize>(
    omega: i32,
    y_bytes: &[u8],
) -> Result<[R; K], &'static str> {
    let omega_u = usize::try_from(omega).expect("Alg 21: omega try_into fail");
    debug_assert!(
        (1..256).contains(&(omega_u + K)),
        "Alg 21: omega+K too large"
    );
    debug_assert_eq!(y_bytes.len(), omega_u + K, "Alg 21: bad output size");

    // 1: h ∈ R^k_2 ∈ ← 0^k
    let mut h: [R; K] = [R0; K];

    // 2: Index ← 0    ▷ Index for reading the first 𝜔 bytes of 𝑦
    let mut index = 0;

    // 3: for i from 0 to k − 1 do    ▷ reconstruct 𝐡[𝑖]
    for i in 0..K {
        //
        // 4: if y[ω + i] < Index or y[ω + i] > ω then return ⊥    ▷ malformed input
        if (y_bytes[omega_u + i] < index) || (y_bytes[omega_u + i] > omega.to_le_bytes()[0]) {
            return Err("Alg 21a: returns ⊥ (4)");

            // 5: end if
        }

        // 6: First ← Index
        let first = index;

        // 7: while Index < y[ω + i] do    ▷ 𝑦[𝜔 + 𝑖] says how far one can advance Index
        while index < y_bytes[omega_u + i] {
            //
            // 8: if Index > First then
            if index > first {
                //
                // 9: if 𝑦[Index − 1] ≥ 𝑦[Index] then return ⊥    ▷ malformed input
                if y_bytes[usize::from(index) - 1] >= y_bytes[usize::from(index)] {
                    return Err("Alg 21a: returns ⊥ (9)");

                    // 10: end if
                }

                // 11: end if
            }
            //
            // 12: h[i]_{y[Index]} ← 1    ▷ 𝑦[Index] says which coefficient in 𝐡[𝑖] should be 1
            h[i].0[y_bytes[index as usize] as usize] = 1;

            // 13: Index ← Index + 1
            index += 1;

            // 14: end while
        }

        // 15: end for
    }

    // 16: for 𝑖 from Index to 𝜔 − 1 do    ▷ read any leftover bytes in the first 𝜔 bytes of 𝑦
    for i in index..omega.to_le_bytes()[0] {
        //
        // 17: if y[i] != 0 then return ⊥
        if y_bytes[i as usize] != 0 {
            return Err("Alg 21b: returns ⊥ (17");

            // 18: end if
        }

        // 19: end for
    }

    debug_assert!(
        h.iter()
            .all(|r| r.0.iter().filter(|&&e| e == 1).sum::<i32>() <= omega),
        "Alg 21: too many 1's in h"
    );

    // 20: return h
    Ok(h)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_core::RngCore;

    #[test]
    fn test_coef_from_three_bytes1() {
        let bytes = [0x12u8, 0x34, 0x56];
        let res = coeff_from_three_bytes::<false>(bytes).unwrap();
        assert_eq!(res, 0x0056_3412);
    }

    #[test]
    fn test_coef_from_three_bytes2() {
        let bytes = [0x12u8, 0x34, 0x80];
        let res = coeff_from_three_bytes::<false>(bytes).unwrap();
        assert_eq!(res, 0x0000_3412);
    }

    #[test]
    fn test_coef_from_three_bytes3() {
        let bytes = [0x01u8, 0xe0, 0x80];
        let res = coeff_from_three_bytes::<false>(bytes).unwrap();
        assert_eq!(res, 0x0000_e001);
    }

    #[test]
    #[should_panic(expected = "panic: out of range")]
    fn test_coef_from_three_bytes4() {
        let bytes = [0x01u8, 0xe0, 0x7f];
        let res = coeff_from_three_bytes::<false>(bytes).expect("panic: out of range");
        assert_eq!(res, 0x0056_3412);
    }

    #[test]
    fn test_coef_from_half_byte1() {
        let inp = 3;
        let res = coeff_from_half_byte::<false>(2, inp).unwrap();
        assert_eq!(-1, res);
    }

    #[test]
    fn test_coef_from_half_byte2() {
        let inp = 8;
        let res = coeff_from_half_byte::<false>(4, inp).unwrap();
        assert_eq!(-4, res);
    }

    #[should_panic]
    #[allow(clippy::should_panic_without_expect)]
    #[test]
    fn test_coef_from_half_byte_validation1() {
        let inp = 22;
        let res = coeff_from_half_byte::<false>(2, inp);
        assert!(res.is_err());
    }

    #[should_panic]
    #[allow(clippy::should_panic_without_expect)]
    #[test]
    fn test_coef_from_half_byte_validation2() {
        let inp = 5;
        let res = coeff_from_half_byte::<false>(1, inp);
        assert!(res.is_err());
    }

    #[test]
    fn test_coef_from_half_byte_validation3() {
        let inp = 10;
        let res = coeff_from_half_byte::<false>(4, inp);
        assert!(res.is_err());
    }

    #[test]
    fn test_simple_bit_pack_roundtrip() {
        // Round trip for 32 * 6(bitlen) bytes
        let mut random_bytes = [0u8; 32 * 6];
        rand::thread_rng().fill_bytes(&mut random_bytes);
        let r = simple_bit_unpack(&random_bytes, (1 << 6) - 1).unwrap();
        let mut res = [0u8; 32 * 6];
        simple_bit_pack(&r, (1 << 6) - 1, &mut res);
        assert_eq!(random_bytes, res);
    }

    #[test]
    #[should_panic]
    #[allow(clippy::should_panic_without_expect)]
    fn test_simple_bit_unpack_validation1() {
        // wrong size of bytes
        let mut random_bytes = [0u8; 32 * 7];
        rand::thread_rng().fill_bytes(&mut random_bytes);
        let res = simple_bit_unpack(&random_bytes, (1 << 6) - 1);
        assert!(res.is_err());
    }

    #[test]
    #[should_panic]
    #[allow(clippy::should_panic_without_expect)]
    fn test_bit_unpack_validation1() {
        // wrong size of bytes
        let mut random_bytes = [0u8; 32 * 7];
        rand::thread_rng().fill_bytes(&mut random_bytes);
        let res = bit_unpack(&random_bytes, 0, (1 << 6) - 1);
        assert!(res.is_err());
    }

    #[test]
    fn test_simple_bit_pack_validation1() {
        let mut random_bytes = [0u8; 32 * 6];
        rand::thread_rng().fill_bytes(&mut random_bytes);
        let r = R([0i32; 256]);
        simple_bit_pack(&r, (1 << 6) - 1, &mut random_bytes);
        // no panic is good news
    }

    #[test]
    #[should_panic(expected = "Alg 16: b out of range")]
    fn test_simple_bit_pack_b_range() {
        let w = R0; //([0i32; 256]);
        let mut bytes = [0u8; 32];
        simple_bit_pack(&w, 0, &mut bytes); // b must be positive
    }

    #[test]
    #[should_panic(expected = "Alg 16: w out of range")]
    fn test_simple_bit_pack_w_range() {
        let mut w = R0; //([0i32; 256]);
        w.0[0] = 5;
        let mut bytes = [0u8; 32];
        simple_bit_pack(&w, 3, &mut bytes); // w coefficient > b
    }

    #[test]
    #[should_panic(expected = "Alg 16: incorrect size of output bytes")]
    fn test_simple_bit_pack_output_size() {
        let w = R0; //([0i32; 256]);
        let mut bytes = [0u8; 65]; // Wrong output size
        simple_bit_pack(&w, 2, &mut bytes);
    }

    #[test]
    #[should_panic(expected = "Alg 17: a out of range")]
    fn test_bit_pack_a_range() {
        let w = R0; //([0i32; 256]);
        let mut bytes = [0u8; 32];
        bit_pack(&w, -1, 2, &mut bytes); // a must be non-negative
    }

    #[test]
    #[should_panic(expected = "Alg 17: b out of range")]
    fn test_bit_pack_b_range() {
        let w = R0; //([0i32; 256]);
        let mut bytes = [0u8; 32];
        bit_pack(&w, 0, 0, &mut bytes); // b must be positive
    }

    #[test]
    #[should_panic(expected = "Alg 17: w out of range")]
    fn test_bit_pack_w_range() {
        let mut w = R0; //([0i32; 256]);
        w.0[0] = 10;
        let mut bytes = [0u8; 32];
        bit_pack(&w, 2, 5, &mut bytes); // w coefficient outside [-a,b] range
    }

    #[test]
    #[should_panic(expected = "Alg 18: b out of range")]
    fn test_simple_bit_unpack_b_range() {
        let bytes = [0u8; 32];
        let _unused = simple_bit_unpack(&bytes, 0); // b must be positive
    }

    #[test]
    #[should_panic(expected = "Alg 18: bad output size")]
    fn test_simple_bit_unpack_input_size() {
        let bytes = [0u8; 65]; // Wrong input size
        let _unused = simple_bit_unpack(&bytes, 2);
    }

    #[test]
    #[should_panic(expected = "Alg 20: omega+K out of range")]
    fn test_hint_bit_pack_omega_k_range() {
        const K: usize = 255;
        let h = [R0; K];
        let mut y_bytes = [0u8; 256];
        hint_bit_pack::<false, K>(2, &h, &mut y_bytes); // omega + K must be < 256
    }

    #[test]
    #[should_panic(expected = "Alg 20: h not 0/1")]
    fn test_hint_bit_pack_h_range() {
        const K: usize = 2;
        let mut h = [R0; K];
        h[0].0[0] = 2; // h must contain only 0s and 1s
        let mut y_bytes = [0u8; 4];
        hint_bit_pack::<false, K>(2, &h, &mut y_bytes);
    }

    #[test]
    #[should_panic(expected = "Alg 21: omega+K too large")]
    fn test_hint_bit_unpack_omega_k_range() {
        const K: usize = 255;
        let y_bytes = [0u8; 256];
        let _unused = hint_bit_unpack::<K>(2, &y_bytes); // omega + K must be < 256
    }
}
