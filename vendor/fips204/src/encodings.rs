// This file implements functionality from FIPS 204 section 7.2 Encodings of ML-DSA Keys and Signatures

use crate::conversion::{
    bit_pack, bit_unpack, hint_bit_pack, hint_bit_unpack, simple_bit_pack, simple_bit_unpack,
};
use crate::helpers::{bit_length, is_in_range};
use crate::types::{R, R0};
use crate::{D, Q};

/// # Algorithm 22: `pkEncode(ρ,t1)` on page 33.
/// Encodes a public key for ML-DSA into a byte string.
///
/// This is only used in `ml_dsa::key_gen()` and does not involve untrusted input.
///
/// **Input**:  `ρ ∈ B^{32}`, `t1 ∈ R^k` with coefficients in `[0, 2^{bitlen(q−1)−d}-1]`. <br>
/// **Output**: Public key `pk ∈ B^{32+32·k·(bitlen(q−1)−d)}`.
pub(crate) fn pk_encode<const K: usize, const PK_LEN: usize>(
    rho: &[u8; 32],
    t1: &[R; K],
) -> [u8; PK_LEN] {
    const BLQD: usize = bit_length(Q - 1) - D as usize;
    debug_assert!(
        t1.iter().all(|t| is_in_range(t, 0, (1 << BLQD) - 1)),
        "Alg 22: t1 out of range"
    );
    debug_assert_eq!(PK_LEN, 32 + 32 * K * BLQD, "Alg 22: bad pk/config size");
    let mut pk = [0u8; PK_LEN];

    // 1: pk ← rho
    pk[0..32].copy_from_slice(rho);

    // 2: for i from 0 to k − 1 do
    // 3: pk ← pk || SimpleBitPack(t1[i], 2^{bitlen(q−1)−d}-1)
    // 4: end for
    pk[32..]
        .chunks_mut(32 * BLQD)
        .enumerate()
        .take(K) // not strictly needed
        .for_each(|(i, chunk)| simple_bit_pack(&t1[i], (1 << BLQD) - 1, chunk));

    // 5: return pk
    pk
}

/// # Algorithm 23: `pkDecode(pk)` on page 33.
/// Reverses the procedure pkEncode.
///
/// Used in `verify_start()` and deserialization with untrusted input. The call to
/// `simple_bit_unpack()` will detect malformed input -- an overly conservative (?) route for now.
///
/// **Input**:  Public key `pk ∈ B^{32+32·k·(bitlen(q−1)−d)}`. <br>
/// **Output**: `ρ ∈ B^{32}`, `t1 ∈ R^k` with coefficients in `[0, 2^{bitlen(q−1)−d}−1]`).
///
/// # Errors
/// Returns an error when the internal `simple_bit_unpack()` invocation finds an element of
/// `t1` is out of range.
pub(crate) fn pk_decode<const K: usize, const PK_LEN: usize>(
    pk: &[u8; PK_LEN],
) -> Result<(&[u8; 32], [R; K]), &'static str> {
    const BLQD: usize = bit_length(Q - 1) - D as usize;
    debug_assert_eq!(pk.len(), 32 + 32 * K * BLQD, "Alg 23: incorrect pk length");
    debug_assert_eq!(PK_LEN, 32 + 32 * K * BLQD, "Alg 23: bad pk/config size");

    // 1: (rho, z_0 , . . . , z_{k−1}) ∈ B^{32} × (B^{32(bitlen(q−1)−d))^k} ← pk
    let rho = <&[u8; 32]>::try_from(&pk[0..32]).expect("Alg 23: try_from fail");

    // 2: for i from 0 to k − 1 do
    let mut t1 = [R0; K]; // cannot use `?` inside a closure
    for i in 0..K {
        //
        // 4: t1[i] ← SimpleBitUnpack(zi, 2^{bitlen(q−1)−d} − 1))    ▷ This is always in the correct range
        t1[i] = simple_bit_unpack(
            &pk[32 + 32 * i * BLQD..32 + 32 * (i + 1) * BLQD],
            (1 << BLQD) - 1,
        )?;
        //
        // 5: end for
    }

    debug_assert!(
        t1.iter().all(|t| is_in_range(t, 0, (1 << BLQD) - 1)),
        "Alg 23: t1 out of range"
    );

    // 6: return (ρ, t1)
    Ok((rho, t1))
}

/// # Algorithm 24: `skEncode(ρ,K,tr,s1,s2,t0)` on page 34.
/// Encodes a secret key for ML-DSA into a byte string.
///
/// This is only used in `ml_dsa::key_gen()` and does not involve untrusted input.
///
/// **Input**: `ρ ∈ B^{32}`, `K ∈ B^{32}`, `tr ∈ B^{64}`,
///            `s_1 ∈ R^l` with coefficients in `[−η, η]`,
///            `s_2 ∈ R^k` with coefficients in `[−η, η]`,
///            `t_0 ∈ R^k` with coefficients in `[−2^{d-1}+1, 2^{d-1}]`.
///             Security parameter `η` (eta) must be either 2 or 4.<br>
/// **Output**: Private key, `sk ∈ B^{32+32+64+32·((k+ℓ)·bitlen(2·η)+d·k)}`
pub(crate) fn sk_encode<const K: usize, const L: usize, const SK_LEN: usize>(
    eta: i32,
    rho: &[u8; 32],
    k: &[u8; 32],
    tr: &[u8; 64],
    s_1: &[R; L],
    s_2: &[R; K],
    t_0: &[R; K],
) -> [u8; SK_LEN] {
    let top = 1 << (D - 1);
    debug_assert!((eta == 2) || (eta == 4), "Alg 24: incorrect eta");
    debug_assert!(
        s_1.iter().all(|x| is_in_range(x, eta, eta)),
        "Alg 24: s1 out of range"
    );
    debug_assert!(
        s_2.iter().all(|x| is_in_range(x, eta, eta)),
        "Alg 24: s2 out of range"
    );
    debug_assert!(
        t_0.iter().all(|x| is_in_range(x, top - 1, top)),
        "Alg 24: t0 out of range"
    );
    debug_assert_eq!(
        SK_LEN,
        128 + 32 * ((K + L) * bit_length(2 * eta) + D as usize * K),
        "Alg 24: bad sk/config size"
    );

    let mut sk = [0u8; SK_LEN];

    // 1: sk ← rho || 𝐾 || tr
    sk[0..32].copy_from_slice(rho);
    sk[32..64].copy_from_slice(k);
    sk[64..128].copy_from_slice(tr);

    // 2: for i from 0 to ℓ − 1 do
    let start = 128;
    let step = 32 * bit_length(2 * eta);
    for i in 0..L {
        //
        // 3: sk ← sk || BitPack (s1[i], η, η)
        bit_pack(
            &s_1[i],
            eta,
            eta,
            &mut sk[start + i * step..start + (i + 1) * step],
        );

        // 4: end for
    }

    // 5: for i from 0 to k − 1 do
    let start = start + L * step;
    for i in 0..K {
        //
        // 6: sk ← sk || BitPack (s2[i], η, η)
        bit_pack(
            &s_2[i],
            eta,
            eta,
            &mut sk[start + i * step..start + (i + 1) * step],
        );

        // 7: end for
    }

    // 8: for i from 0 to k − 1 do
    let start = start + K * step;
    let step = 32 * D as usize;
    for i in 0..K {
        //
        // 9: sk ← sk || BitPack (t0[i], [−2^{d-1} + 1, 2^{d-1}] )
        bit_pack(
            &t_0[i],
            top - 1,
            top,
            &mut sk[start + i * step..start + (i + 1) * step],
        );

        // 10: end for
    }

    // ...just make sure we really hit the end of the sk slice
    debug_assert_eq!(start + K * step, sk.len(), "Alg 24: length miscalc");

    // 11: return sk
    sk
}

/// # Algorithm 25: `skDecode(sk)` on page 34.
/// Reverses the procedure in `skEncode()`.
///
/// Used in `sign_start()` and deserialization with untrusted input.
///
/// **Input**:  Private key, `sk ∈ B^{32+32+64+32·((ℓ+k)·bitlen(2η)+d·k)}`
///             Security parameter `η` (eta) must be either 2 or 4.<br>
/// **Output**: `ρ ∈ B^{32}`, `K ∈ B^{32}`, `tr ∈ B^{64}`,
///             `s_1 ∈ R^ℓ`, `s_2 ∈ R^k`, `t_0 ∈ R^k` with coefficients in `[−2^{d−1}+1, 2^{d−1}]`.
///
/// # Errors
/// Returns an error when any of the output coefficients are out of range. <br>
#[allow(clippy::similar_names, clippy::type_complexity)]
pub(crate) fn sk_decode<const K: usize, const L: usize, const SK_LEN: usize>(
    eta: i32,
    sk: &[u8; SK_LEN],
) -> Result<(&[u8; 32], &[u8; 32], &[u8; 64], [R; L], [R; K], [R; K]), &'static str> {
    const TOP: i32 = 1 << (D - 1);
    debug_assert!((eta == 2) || (eta == 4), "Alg 25: incorrect eta");
    debug_assert_eq!(
        SK_LEN,
        128 + 32 * ((K + L) * bit_length(2 * eta) + D as usize * K),
        "Alg 25: bad sk/config size"
    );
    let (mut s_1, mut s_2, mut t_0) = ([R0; L], [R0; K], [R0; K]);

    // 1: (rho, 𝐾, tr, 𝑦0 , … , 𝑦ℓ−1 , 𝑧0 , … , 𝑧𝑘−1 , 𝑤0 , … , 𝑤𝑘−1 ) ∈
    //    B^32 × B^32 × B^64 × B^{32·bitlen(2η)}^l × B^{32·bitlen(2η)}^k × B^{32d}^k ← sk
    let rho = <&[u8; 32]>::try_from(&sk[0..32]).expect("Alg 25: try_from1 fail");
    let k = <&[u8; 32]>::try_from(&sk[32..64]).expect("Alg 25: try_from2 fail");
    let tr = <&[u8; 64]>::try_from(&sk[64..128]).expect("Alg 25: try_from3 fail");
    // y & z unpack is done inline below...

    // 2: for i from 0 to ℓ − 1 do
    let start = 128;
    let step = 32 * bit_length(2 * eta);
    for i in 0..L {
        //
        // 3: s1[i] ← BitUnpack(yi, η, η)   ▷ This may lie outside [−η, η], if input is malformed
        s_1[i] = bit_unpack(&sk[start + i * step..start + (i + 1) * step], eta, eta)?;

        // 4: end for
    }

    // 5: for i from 0 to k − 1 do
    let start = start + L * step;
    for i in 0..K {
        //
        // 6: s2[i] ← BitUnpack(zi, η, η) ▷ This may lie outside [−η, η], if input is malformed
        s_2[i] = bit_unpack(&sk[start + i * step..start + (i + 1) * step], eta, eta)?;

        // 7: end for
    }

    // 8: for i from 0 to k − 1 do
    let start = start + K * step;
    let step = 32 * D as usize;
    for i in 0..K {
        //
        // 9: t0[i] ← BitUnpack(wi, −2^{d−1} - 1, 2^{d−1})   ▷ This is always in the correct range
        t_0[i] = bit_unpack(&sk[start + i * step..start + (i + 1) * step], TOP - 1, TOP)?;

        // 10: end for
    }

    // ... just make sure we hit the end of sk slice properly
    debug_assert_eq!(start + K * step, sk.len(), "Alg 25: length miscalc");

    // 11: return (pho, 𝐾, tr, s1, s2, t0 )
    Ok((rho, k, tr, s_1, s_2, t_0))
}

/// # Algorithm 26: `sigEncode(c_tilde,z,h)` on page 35.
/// Encodes a signature into a byte string.
///
/// This is only used in `ml_dsa::sign_finish()` and is not exposed to untrusted input.
/// The `CTEST` generic is only passed through to the `hint_bit_pack()` leaf function
/// such that this logic becomes constant-time.
///
/// **Input**: `c_tilde ∈ B^{λ/4}`,
///            `z ∈ R^ℓ` with coefficients in `[−1*γ_1 + 1, γ_1]`,
///            `h ∈ R^k_2`. <br>
/// **Output**: Signature, `σ ∈ B^{λ/4+l·32·(1+bitlen(γ_1-1)+ω+k}`
pub(crate) fn sig_encode<
    const CTEST: bool,
    const K: usize,
    const L: usize,
    const LAMBDA_DIV4: usize,
    const SIG_LEN: usize,
>(
    gamma1: i32,
    omega: i32,
    c_tilde: &[u8; LAMBDA_DIV4],
    z: &[R; L],
    h: &[R; K],
) -> [u8; SIG_LEN] {
    debug_assert!(
        z.iter().all(|x| is_in_range(x, gamma1 - 1, gamma1)),
        "Alg 26: z out of range"
    );
    debug_assert!(
        h.iter().all(|x| is_in_range(x, 0, 1)),
        "Alg 26: h out of range"
    );
    debug_assert_eq!(
        SIG_LEN,
        LAMBDA_DIV4 + L * 32 * (1 + bit_length(gamma1 - 1)) + omega.unsigned_abs() as usize + K,
        "Alg 26: bad sig/config size"
    );

    let mut sigma = [0u8; SIG_LEN];

    // 1: sigma ← c_tilde
    sigma[..LAMBDA_DIV4].copy_from_slice(c_tilde);

    // 2: for i from 0 to ℓ − 1 do
    let start = LAMBDA_DIV4;
    let step = 32 * (1 + bit_length(gamma1 - 1));
    for i in 0..L {
        //
        // 3: σ ← σ || BitPack (z[i], γ_1 − 1, γ_1)  (note: this checks the range of z)
        bit_pack(
            &z[i],
            gamma1 - 1,
            gamma1,
            &mut sigma[start + i * step..start + (i + 1) * step],
        );

        // 4: end for
    }

    // 5: σ ← σ || HintBitPack (h)
    hint_bit_pack::<CTEST, K>(omega, h, &mut sigma[start + L * step..]);

    // 6: return 𝜎
    sigma
}

/// # Algorithm 27: `sigDecode(σ)` on page 35.
/// Reverses the procedure `sigEncode()`.
///
/// Used in `verify_finish()` with untrusted input.
///
/// **Input**:  Signature, `σ ∈ B^{λ/4+ℓ·32·(1+bitlen(γ_1-1))+ω+k` <br>
/// **Output**: `c_tilde ∈ B^{λ/4}`,
///             `z ∈ R^ℓ` with coefficients in `[−γ_1 + 1, γ_1]`,
///             `h ∈ R^k_2` or `⊥`. <br>
///
/// # Errors
/// Returns an error when decoded coefficients fall out of range.
#[allow(clippy::type_complexity)]
pub(crate) fn sig_decode<
    const K: usize,
    const L: usize,
    const LAMBDA_DIV4: usize,
    const SIG_LEN: usize,
>(
    gamma1: i32,
    omega: i32,
    sigma: &[u8; SIG_LEN],
) -> Result<([u8; LAMBDA_DIV4], [R; L], Option<[R; K]>), &'static str> {
    debug_assert_eq!(
        SIG_LEN,
        LAMBDA_DIV4 + L * 32 * (1 + bit_length(gamma1 - 1)) + omega.unsigned_abs() as usize + K,
        "Alg 27: bad sig/config size"
    );

    let mut c_tilde = [0u8; LAMBDA_DIV4];
    let mut z: [R; L] = [R0; L];

    // 1: (ω, x_0, ... , x_{ℓ−1}, y) ∈ B^{λ/4} × Bℓ·32·(1+bitlen(γ_1−1))+ω+k ← σ
    c_tilde[0..LAMBDA_DIV4].copy_from_slice(&sigma[0..LAMBDA_DIV4]);

    // 2: for i from 0 to ℓ − 1 do
    let start = LAMBDA_DIV4;
    let step = 32 * (bit_length(gamma1 - 1) + 1);
    for i in 0..L {
        //
        // 3: z[i] ← BitUnpack(xi, γ1 − 1, γ1)    ▷ This is always in the correct range, as γ1 is a power of 2
        z[i] = bit_unpack(
            &sigma[start + i * step..start + (i + 1) * step],
            gamma1 - 1,
            gamma1,
        )?;

        // 4: end for
    }

    // 5: h ← HintBitUnpack(y)
    let h = hint_bit_unpack::<K>(omega, &sigma[start + L * step..])?;

    // 6: return (c_tilde, z, h)  -- note h is never really returned as None per result on above line
    Ok((c_tilde, z, Some(h)))
}

/// # Algorithm 28: `w1Encode(w1)` on page 35.
/// Encodes a polynomial vector `w1` into a bit string.
///
/// Used in `ml_dsa::sign_finish()` and `ml_dsa::verify_finish()`, and not exposed to untrusted input.
///
/// **Input**: `w1 ∈ R^k` with coefficients in `[0, (q − 1)/(2γ_2) − 1]`.
/// **Output**: A bit string representation, `w1_tilde ∈ {0,1}^{32·k·bitlen((q-1)/(2γ2)−1)}`.
pub(crate) fn w1_encode<const K: usize>(gamma2: i32, w1: &[R; K], w1_tilde: &mut [u8]) {
    let qm1_d_2g_m1 = (Q - 1) / (2 * gamma2) - 1;
    debug_assert_eq!(
        w1_tilde.len(),
        32 * K * bit_length(qm1_d_2g_m1),
        "Alg 28: bad w1_tilde/config size"
    );
    debug_assert!(
        w1.iter().all(|r| is_in_range(r, 0, qm1_d_2g_m1)),
        "Alg 28: w1 out of range"
    );

    // 1: w1_tilde ← ()

    // 2: for i from 0 to k − 1 do
    let step = 32 * bit_length(qm1_d_2g_m1);
    for i in 0..K {
        //
        // 3: w1_tilde ← w1_tilde || BytesToBits (SimpleBitPack (w1[i], (q − 1)/(2γ2) − 1))
        simple_bit_pack(&w1[i], qm1_d_2g_m1, &mut w1_tilde[i * step..(i + 1) * step]);

        // 4: end for
    }

    // 5: return w1_tilde
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_core::RngCore;

    #[test]
    fn test_pk_encode_decode_roundtrip1() {
        // D=13 K=4 PK_LEN=1312
        let mut random_pk = [0u8; 1312];
        random_pk.iter_mut().for_each(|a| *a = rand::random::<u8>());
        //let mut rho = [0u8; 32];
        //let mut t1 = [[0i32; 256]; 4];
        let (rho, t1) = pk_decode::<4, 1312>(&random_pk).unwrap();
        //let mut res = [0u8; 1312];
        let res = pk_encode::<4, 1312>(rho, &t1);
        assert_eq!(&random_pk[..], res);
    }

    #[test]
    fn test_pk_encode_decode_roundtrip2() {
        // D=13 K=6 PK_LEN=1952
        let mut random_pk = [0u8; 1952];
        random_pk.iter_mut().for_each(|a| *a = rand::random::<u8>());
        //let mut rho = [0u8; 32];
        //let mut t1 = [[0i32; 256]; 6];
        let (rho, t1) = pk_decode::<6, 1952>(&random_pk).unwrap();
        //let mut res = [0u8; 1952];
        let res = pk_encode::<6, 1952>(rho, &t1);
        assert_eq!(random_pk, res);
    }

    #[test]
    fn test_pk_encode_decode_roundtrip3() {
        // D=13 K=8 PK_LEN=2592
        let mut random_pk = [0u8; 2592];
        random_pk.iter_mut().for_each(|a| *a = rand::random::<u8>());
        //let mut rho = [0u8; 32];
        //let mut t1 = [[0i32; 256]; 8];
        let (rho, t1) = pk_decode::<8, 2592>(&random_pk).unwrap();
        //let mut res = [0u8; 2592];
        let res = pk_encode::<8, 2592>(rho, &t1);
        assert_eq!(random_pk, res);
    }

    fn get_vec(max: u32) -> R {
        let mut rnd_r = R0; //[0i32; 256];
        rnd_r
            .0
            .iter_mut()
            .for_each(|e| *e = rand::random::<i32>().rem_euclid(i32::try_from(max).unwrap()));
        rnd_r
    }

    #[test]
    #[allow(clippy::similar_names)]
    fn test_sk_encode_decode_roundtrip1() {
        // D=13 ETA=2 K=4 L=4 SK_LEN=2560
        let (rho, k) = (rand::random::<[u8; 32]>(), rand::random::<[u8; 32]>());
        let mut tr = [0u8; 64];
        tr.iter_mut().for_each(|e| *e = rand::random::<u8>());
        let s1 = [get_vec(2), get_vec(2), get_vec(2), get_vec(2)];
        let s2 = [get_vec(2), get_vec(2), get_vec(2), get_vec(2)];
        let t0 = [
            get_vec(1 << 11),
            get_vec(1 << 11),
            get_vec(1 << 11),
            get_vec(1 << 11),
        ];
        //let mut sk = [0u8; 2560];
        let sk = sk_encode::<4, 4, 2560>(2, &rho, &k, &tr, &s1, &s2, &t0);
        let res = sk_decode::<4, 4, 2560>(2, &sk);
        assert!(res.is_ok());
        let (rho_test, k_test, tr_test, s1_test, s2_test, t0_test) = res.unwrap();

        assert!(
            (rho == *rho_test)
                && (k == *k_test)
                && (tr == *tr_test)
                && (s1.iter().zip(s1_test.iter()).all(|(a, b)| a.0 == b.0))
                && (s2.iter().zip(s2_test.iter()).all(|(a, b)| a.0 == b.0))
                && (t0.iter().zip(t0_test.iter()).all(|(a, b)| a.0 == b.0))
        );
    }

    #[test]
    fn test_sig_roundtrip() {
        // GAMMA1=2^17 K=4 L=4 LAMBDA=128 OMEGA=80
        let mut c_tilde = [0u8; 2 * 128 / 8];
        rand::thread_rng().fill_bytes(&mut c_tilde);
        let z = [get_vec(2), get_vec(2), get_vec(2), get_vec(2)];
        let h = [get_vec(1), get_vec(1), get_vec(1), get_vec(1)];
        let sigma =
            sig_encode::<false, 4, 4, { 128 / 4 }, 2420>(1 << 17, 80, &c_tilde.clone(), &z, &h);
        let (c_test, z_test, h_test) =
            sig_decode::<4, 4, { 128 / 4 }, 2420>(1 << 17, 80, &sigma).unwrap();
        assert_eq!(c_tilde[0..8], c_test[0..8]);
        assert!(z.iter().zip(z_test.iter()).all(|(a, b)| a.0 == b.0));
        assert!(h
            .iter()
            .zip(h_test.unwrap().iter())
            .all(|(a, b)| a.0 == b.0));
    }
}
