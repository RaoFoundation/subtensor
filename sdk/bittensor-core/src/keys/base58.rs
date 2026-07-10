//! Fast base58 encoding for ss58 rendering.
//!
//! The generic byte-at-a-time base58 algorithm (bs58, used by sp-core) costs
//! ~1.6µs per address and dominates every decode path that renders account
//! ids — metagraph payloads and storage-map pages render thousands per call.
//! This encoder works in u32 limbs and extracts five digits per big-integer
//! division (58^5 is the largest power of 58 below 2^32), which is ~6x
//! faster. Equivalence with sp-core's rendering is pinned by tests.

const ALPHABET: &[u8; 58] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

/// 58^5: five base-58 digits per division.
const POW5: u64 = 656_356_768;

/// Divide the big-endian u32-limb integer in place; returns the remainder.
///
/// The quotient limb always fits: `acc < divisor << 32` because
/// `rem < divisor`, so `acc / divisor < 2^32`.
fn div_rem(limbs: &mut [u32], divisor: u64) -> u64 {
    let mut rem: u64 = 0;
    for limb in limbs.iter_mut() {
        let acc = (rem << 32) | u64::from(*limb);
        *limb = (acc / divisor) as u32;
        rem = acc % divisor;
    }
    rem
}

/// Base58-encode `input` (big-endian byte string, bitcoin alphabet).
pub fn base58_encode(input: &[u8]) -> String {
    let zeros = input.iter().take_while(|&&b| b == 0).count();
    let bytes = &input[zeros..];

    // Pack big-endian into u32 limbs; the first limb may be partial.
    let mut limbs: Vec<u32> = Vec::with_capacity(bytes.len().div_ceil(4));
    let head = bytes.len() % 4;
    if head != 0 {
        let mut limb = 0u32;
        for &b in &bytes[..head] {
            limb = (limb << 8) | u32::from(b);
        }
        limbs.push(limb);
    }
    for chunk in bytes[head..].chunks_exact(4) {
        limbs.push(u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }

    // Little-endian base-58 digits.
    let mut digits: Vec<u8> = Vec::with_capacity(bytes.len() * 137 / 100 + 1);
    let mut window: &mut [u32] = &mut limbs;
    loop {
        // Skip exhausted high limbs.
        let nonzero = window.iter().position(|&l| l != 0);
        let Some(start) = nonzero else { break };
        window = &mut window[start..];
        let mut rem = div_rem(window, POW5);
        let exhausted = window.iter().all(|&l| l == 0);
        if exhausted {
            // Last chunk: no leading zero digits.
            while rem > 0 {
                digits.push((rem % 58) as u8);
                rem /= 58;
            }
        } else {
            for _ in 0..5 {
                digits.push((rem % 58) as u8);
                rem /= 58;
            }
        }
    }

    let mut out = String::with_capacity(zeros + digits.len());
    for _ in 0..zeros {
        out.push('1');
    }
    for &d in digits.iter().rev() {
        out.push(ALPHABET[d as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn matches_bs58_reference() {
        let cases: Vec<Vec<u8>> = vec![
            vec![],
            vec![0],
            vec![0, 0, 0],
            vec![1],
            vec![57],
            vec![58],
            vec![255; 35],
            vec![0, 0, 1, 2, 3],
            (0..=255u8).collect(),
        ];
        for case in cases {
            assert_eq!(
                base58_encode(&case),
                bs58::encode(&case).into_string(),
                "diverged for {case:?}"
            );
        }
        // Randomized 35/36-byte inputs (the ss58 shapes).
        let mut state = 0x9e37_79b9_7f4a_7c15u64;
        for _ in 0..500 {
            for len in [35usize, 36] {
                let mut data = vec![0u8; len];
                for b in &mut data {
                    state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                    *b = (state >> 33) as u8;
                }
                assert_eq!(base58_encode(&data), bs58::encode(&data).into_string());
            }
        }
    }
}
