#![no_std]
#![deny(clippy::pedantic, warnings, missing_docs, unsafe_code)]
// Preserve the upstream source review surface across newer Clippy releases.
#![allow(clippy::needless_for_each, clippy::unnecessary_semicolon)]
// Almost all of the 'allow' category...
#![deny(absolute_paths_not_starting_with_crate, dead_code)]
#![deny(
    elided_lifetimes_in_paths,
    explicit_outlives_requirements,
    keyword_idents
)]
#![deny(
    let_underscore_drop,
    macro_use_extern_crate,
    meta_variable_misuse,
    missing_abi
)]
#![deny(non_ascii_idents, rust_2021_incompatible_closure_captures)]
#![deny(
    rust_2021_incompatible_or_patterns,
    rust_2021_prefixes_incompatible_syntax
)]
#![deny(rust_2021_prelude_collisions, single_use_lifetimes, trivial_casts)]
#![deny(
    trivial_numeric_casts,
    unreachable_pub,
    unsafe_op_in_unsafe_fn,
    unstable_features
)]
#![deny(
    unused_extern_crates,
    unused_import_braces,
    unused_lifetimes,
    unused_macro_rules
)]
#![deny(unused_qualifications, unused_results, variant_size_differences)]
//
#![doc = include_str!("../README.md")]

// TODO Roadmap
//  1. Always more testing...
//  2. Performance optimizations

// Implements FIPS 204 Module-Lattice-Based Digital Signature Standard.
// See <https://nvlpubs.nist.gov/nistpubs/FIPS/NIST.FIPS.204.pdf>

// Functionality map per FIPS 204
//
// Algorithm 1 ML-DSA.KeyGen() on page 17                   --> from lib.rs to ml_dsa.rs
// Algorithm 2 ML-DSA.Sign(sk,M,ctx) on page 18             --> lib.rs
// Algorithm 3 ML-DSA.Verify(pk,M,s,ctx) on page 18         --> lib.rs
// Algorithm 4 HashML-DSA.Sign(sk,M,ctx,PH) on page 20      --> lib.rs
// Algorithm 5 HashML-DSA.Verify(sk,M,s,ctx,PH) on page 21  --> lib.rs
// Algorithm 6 ML-DSA.KeyGen_internal(x) on page 23         --> ml_dsa.rs
// Algorithm 7 ML-DSA.Sign_internal(sk,M',rnd) on page 25   --> ml_dsa.rs
// Algorithm 8 ML-DSA.Verify_internal(pk,M',s) on page 27   --> ml_dsa.rs
// Algorithm 9 IntegerToBits(x,a) one page 28               --> (optimized away) conversion.rs
// Algorithm 10 BitsToInteger(y,a) on page 28               --> (optimized away) conversion.rs
// Algorithm 11 IntegerToBytes(x,a) on page 28              --> (optimized away) conversion.rs
// Algorithm 12 BitsToBytes(y) on page 29                   --> (optimized away) conversion.rs
// Algorithm 13 BytesToBits(z) on page 29                   --> (optimized away) conversion.rs
// Algorithm 14 CoefFromThreeBytes(b0,b1,b2) on page 29     --> conversion.rs
// Algorithm 15 CoefFromHalfByte(b) on page 30              --> conversion.rs
// Algorithm 16 SimpleBitPack(w,b) on page 30               --> conversion.rs
// Algorithm 17 BitPack(w,a,b) on page 30                   --> conversion.rs
// Algorithm 18 SimpleBitUnpack(v,b) on page 31             --> conversion.rs
// Algorithm 19 BitUnpack(v,a,b) on page 31                 --> conversion.rs
// Algorithm 20 HintBitPack(h) on page 32                   --> conversion.rs
// Algorithm 21 HintBitUnpack(y) on page 32                 --> conversion.rs
// Algorithm 22 pkEncode(ρ,t1) on page 33                   --> encodings.rs
// Algorithm 23 pkDecode(pk) on page 33                     --> encodings.rs
// Algorithm 24 skEncode(ρ,K,tr,s1,s2,t0) on page 34        --> encodings.rs
// Algorithm 25 skDecode(sk) on page 34                     --> encodings.rs
// Algorithm 26 sigEncode(c˜,z,h) on page 35                --> encodings.rs
// Algorithm 27 sigDecode(σ) on page 35                     --> encodings.rs
// Algorithm 28 w1Encode(w1) on page 35                     --> encodings.rs
// Algorithm 29 SampleInBall(ρ) on page 36                  --> hashing.rs
// Algorithm 30 RejNTTPoly(ρ) on page 37                    --> hashing.rs
// Algorithm 31 RejBoundedPoly(ρ) on page 37                --> hashing.rs
// Algorithm 32 ExpandA(ρ) on page 38                       --> hashing.rs
// Algorithm 33 ExpandS(ρ) on page 38                       --> hashing.rs
// Algorithm 34 ExpandMask(ρ,µ) on page 38                  --> hashing.rs
// Algorithm 35 Power2Round(r) on page 40                   --> high_low.rs
// Algorithm 36 Decompose(r) on page 40                     --> high_low.rs
// Algorithm 37 HighBits(r) on page 40                      --> high_low.rs
// Algorithm 38 LowBits(r) on page 41                       --> high_low.rs
// Algorithm 39 MakeHint(z,r) on page 41                    --> high_low.rs
// Algorithm 40 UseHint(h,r) on page 41                     --> high_low.rs
// Algorithm 41 NTT(w) on page 43                           --> ntt.rs
// Algorithm 42 NTT−1(wˆ) on page 44                        --> ntt.rs
// Algorithm 43 BitRev8(m) on page 44                       --> not needed to to zeta table
// Algorithm 44 AddNTT(a,b)̂ on page 45                      --> helpers.rs within 46:AddVectorNTT
// Algorithm 45 MultiplyNTT(a,b)̂ on page 45                 --> helpers.rs
// Algorithm 46 AddVectorNTT(v,w) on page 45                --> helpers.rs
// Algorithm 47 ScalarVectorNTT(c,v)̂ on page 46             --> not implemented standalone
// Algorithm 48 MatrixVectorNTT(M,v) on page 46             --> not implemented standalone
// Algorithm 49 MontgomeryReduce(a) on page 50              --> helpers.rs
// Types are in types.rs, traits are in traits.rs...

// Note that debug_assert! statements enforce correct program construction and are not involved
// in any operational dataflow (so are good fuzz targets). The ensure! statements implement
// conservative dataflow validation and do not panic. Separately, functions are only generic
// over security parameters that are directly involved in memory allocation (on the stack).
// Some coding oddities are driven by 'clippy pedantic' and the fact that Rust doesn't currently
// do well with arithmetic on generic parameters.

// Note that the `CTEST` generic parameter supports constant-time measurements by dudect. This
// is done by minimally removing timing variability of non-secret data (such as the rejection
// sampling of hash derived from rho). All normal crate functionality has this disabled (set to
// `false`) except for the single function (per namespace) `dudect_keygen_sign_with_rng()`
// which is only exposed when the non-default `dudect` feature is enabled.

/// The `rand_core` types are re-exported so that users of fips204 do not
/// have to worry about using the exact correct version of `rand_core`.
pub use rand_core::{CryptoRng, Error as RngError, RngCore};

mod conversion;
mod encodings;
mod hashing;
mod helpers;
mod high_low;
mod ml_dsa;
mod ntt;
mod types;

/// All functionality is covered by traits, such that consumers can utilize trait objects as desired.
pub mod traits;
pub use crate::types::Ph;

// Applies across all security parameter sets
const Q: i32 = 8_380_417; // 2^23 - 2^13 + 1 = 0x7FE001; page 15 table 1 first row
const ZETA: i32 = 1753; // See section 2.5 of FIPS 204; page 15 table 1 second row
const D: u32 = 13; // See page 15 table 1 third row

// This common functionality is injected into each security parameter set namespace, and is
// largely a lightweight wrapper into the ml_dsa functions.
macro_rules! functionality {
    () => {
        use crate::encodings;
        use crate::hashing;
        use crate::helpers;
        use crate::ml_dsa;
        use crate::ntt;
        use crate::traits::{KeyGen, SerDes, Signer, Verifier};
        use crate::types;
        use rand_core::CryptoRngCore;
        use zeroize::{Zeroize, ZeroizeOnDrop};

        use crate::{D, Q};
        const BETA: i32 = TAU * ETA;
        const LAMBDA_DIV4: usize = LAMBDA / 4;
        const W1_LEN: usize = 32 * K * helpers::bit_length((Q - 1) / (2 * GAMMA2) - 1);
        const CTEST: bool = false; // When true, the logic goes into CT test mode


        // ----- 'EXTERNAL' DATA TYPES -----

        /// Empty struct to enable `KeyGen` trait objects across security parameter
        /// sets. Implements the [`crate::traits::KeyGen`] trait.
        #[derive(Zeroize, ZeroizeOnDrop)]
        pub struct KG();


        /// Private key specific to the target security parameter set that contains
        /// precomputed elements which improves signature performance.
        ///
        /// Implements the [`crate::traits::Signer`] and [`crate::traits::SerDes`] traits.
        // Note: #[derive(Zeroize, ZeroizeOnDrop)] is implemented on the underlying struct.
        pub type PrivateKey = crate::types::PrivateKey<K, L>;


        /// Public key specific to the target security parameter set that contains
        /// precomputed elements which improves verification performance.
        ///
        /// Implements the [`crate::traits::Verifier`] and [`crate::traits::SerDes`] traits.
        // Note: #[derive(Zeroize, ZeroizeOnDrop)] is implemented on the underlying struct.
        pub type PublicKey = crate::types::PublicKey<K, L>;


        // Note: (public) Signature is just a vanilla fixed-size byte array


        // ----- PRIMARY FUNCTIONS ---

        /// # Algorithm 1: `ML-DSA.KeyGen()` on page 17.
        /// Generates a public-private key pair specific to this security parameter set.
        ///
        /// This function utilizes the **default OS ** random number generator. It operates
        /// in constant-time relative to secret data (which specifically excludes the
        /// random number generator internals, the `rho` value stored in the public key,
        /// and the hash-derived `rho_prime` value that is rejection-sampled/expanded into
        /// the internal `s_1` and `s_2` values).
        ///
        /// **Output**: Public key struct and private key struct.
        ///
        /// # Errors
        /// Returns an error if the random number generator fails.
        ///
        /// # Examples
        /// ```rust
        /// # use std::error::Error;
        /// # fn main() -> Result<(), Box<dyn Error>> {
        /// # #[cfg(all(feature = "ml-dsa-44", feature = "default-rng"))] {
        /// use fips204::ml_dsa_44; // Could also be ml_dsa_65 or ml_dsa_87.
        /// use fips204::traits::{SerDes, Signer, Verifier};
        ///
        /// let message = [0u8, 1, 2, 3, 4, 5, 6, 7];
        ///
        /// // Generate key pair and signature
        /// let (pk1, sk) = ml_dsa_44::try_keygen()?;  // Generate both public and secret keys
        /// let sig1 = sk.try_sign(&message, &[0])?;  // Use the secret key to generate a message signature
        /// # }
        /// # Ok(())}
        /// ```
        #[cfg(feature = "default-rng")]
        pub fn try_keygen() -> Result<(PublicKey, PrivateKey), &'static str> { KG::try_keygen() }


        /// # Algorithm 1: `ML-DSA.KeyGen()` on page 17.
        /// Generates a public and private key pair specific to this security parameter set.
        ///
        /// This function utilizes the **provided** random number generator. It operates
        /// in constant-time relative to secret data (which specifically excludes the
        /// random number generator internals, the `rho` value stored in the public key,
        /// and the hash-derived `rho_prime` value that is rejection-sampled/expanded into
        /// the internal `s_1` and `s_2` values).
        ///
        /// **Output**: Public key struct and private key struct.
        ///
        /// # Errors
        /// Returns an error if the random number generator fails.
        ///
        /// # Examples
        /// ```rust
        /// # use std::error::Error;
        /// # fn main() -> Result<(), Box<dyn Error>> {
        /// # #[cfg(feature = "ml-dsa-44")] {
        /// use fips204::ml_dsa_44; // Could also be ml_dsa_65 or ml_dsa_87.
        /// use fips204::traits::{SerDes, Signer, Verifier};
        /// use rand_chacha::rand_core::SeedableRng;
        ///
        /// let message = [0u8, 1, 2, 3, 4, 5, 6, 7];
        /// let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(123);
        ///
        /// // Generate key pair and signature
        /// let (pk1, sk) = ml_dsa_44::try_keygen_with_rng(&mut rng)?;  // Generate both public and secret keys
        /// let sig1 = sk.try_sign_with_rng(&mut rng, &message, &[0])?;  // Use the secret key to generate a message signature
        /// # }
        /// # Ok(())}
        /// ```
        pub fn try_keygen_with_rng(rng: &mut impl CryptoRngCore) -> Result<(PublicKey, PrivateKey), &'static str> {
            KG::try_keygen_with_rng(rng)
        }


        impl KeyGen for KG {
            type PrivateKey = PrivateKey;
            type PublicKey = PublicKey;


            /// # Algorithm 1 in `KeyGen` trait
            fn try_keygen_with_rng(rng: &mut impl CryptoRngCore) -> Result<(PublicKey, PrivateKey), &'static str> {
                let (pk, sk) = ml_dsa::key_gen::<CTEST, K, L, PK_LEN, SK_LEN>(rng, ETA)?;
                Ok((pk, sk))
            }

            /// # Algorithm 1 in `KeyGen` trait
            fn keygen_from_seed(xi: &[u8; 32]) -> (Self::PublicKey, Self::PrivateKey) {
                let (pk, sk) = ml_dsa::key_gen_internal::<CTEST, K, L, PK_LEN, SK_LEN>(ETA, xi);
                (pk, sk)
            }
        }


        impl Signer for PrivateKey {
            type Signature = [u8; SIG_LEN];
            type PublicKey = PublicKey;

            /// # Algorithm 2: `ML-DSA.Sign(sk, 𝑀 , ctx)` on page 18.
            /// Generates an ML-DSA signature.
            ///
            /// **Input**:  Implemented on private key struct,
            ///             message `𝑀 ∈ {0, 1}∗`,
            ///             context string `ctx` (a byte string of 255 or fewer bytes). <br>
            /// **Output**: Signature `𝜎 ∈ 𝔹𝜆/4+ℓ⋅32⋅(1+bitlen (𝛾1−1))+𝜔+𝑘`.
            ///
            /// # Errors
            /// Returns an error when the random number generator fails or context too long.
            fn try_sign_with_rng(
                &self, rng: &mut impl CryptoRngCore, message: &[u8], ctx: &[u8],
            ) -> Result<Self::Signature, &'static str> {
                // 1: if |ctx| > 255 then
                // 2:   return ⊥    ▷ return an error indication if the context string is too long
                // 3: end if
                helpers::ensure!(ctx.len() < 256, "ML-DSA.Sign: ctx too long");

                // 4:  (blank line in spec)

                // 5: rnd ← 𝔹^{32}     ▷ for the optional deterministic variant, substitute rnd ← {0}^32
                // 6: if rnd = NULL then
                // 7:   return ⊥    ▷ return an error indication if random bit generation failed
                // 8: end if
                let mut rnd = [0u8; 32];
                rng.try_fill_bytes(&mut rnd).map_err(|_| "ML-DSA.Sign: random number generator failed")?;

                // 9:  (blank line in spec)

                // Note: step 10 is done within sign_internal() and 'below'
                // 10: 𝑀 ′ ← BytesToBits(IntegerToBytes(0, 1) ∥ IntegerToBytes(|𝑐𝑡𝑥|, 1) ∥ 𝑐𝑡𝑥) ∥ 𝑀
                // 11: 𝜎 ← ML-DSA.Sign_internal(𝑠𝑘, 𝑀 ′ , 𝑟𝑛𝑑)
                let sig = ml_dsa::sign_internal::<CTEST, K, L, LAMBDA_DIV4, SIG_LEN, SK_LEN, W1_LEN>(
                    BETA, GAMMA1, GAMMA2, OMEGA, TAU, &self, message, ctx, &[], &[], rnd, false
                );

                // 12: return 𝜎
                Ok(sig)
            }


            /// # Algorithm 4: `HashML-DSA.Sign(𝑠𝑘, 𝑀 , 𝑐𝑡𝑥, PH)` on page 20.
            /// Generate a “pre-hash” ML-DSA signature.
            ///
            /// **Input**:  Implemented on private key struct,
            ///             message `𝑀 ∈ {0, 1}∗`,
            ///             context string `ctx` (a byte string of 255 or fewer bytes),
            ///             pre-hash function `PH`. <br>
            /// **Output**: ML-DSA signature `𝜎 ∈ 𝔹^{𝜆/4+ℓ⋅32⋅(1+bitlen(𝛾1 −1))+𝜔+𝑘}`.
            ///
            /// # Errors
            /// Returns an error when the random number generator fails or context too long.
            fn try_hash_sign_with_rng(
                &self, rng: &mut impl CryptoRngCore, message: &[u8], ctx: &[u8], ph: &types::Ph,
            ) -> Result<Self::Signature, &'static str> {
                // 1: if |ctx| > 255 then
                // 2:   return ⊥    ▷ return an error indication if the context string is too long
                // 3: end if
                helpers::ensure!(ctx.len() < 256, "HashML-DSA.Sign: ctx too long");

                // 4:  (blank line in spec)

                // 5: rnd ← 𝔹^{32}     ▷ for the optional deterministic variant, substitute rnd ← {0}^32
                // 6: if rnd = NULL then
                // 7:   return ⊥    ▷ return an error indication if random bit generation failed
                // 8: end if
                let mut rnd = [0u8; 32];
                rng.try_fill_bytes(&mut rnd).map_err(|_| "HashML-DSA.Sign: random number generator failed")?;

                // 9:  (blank line in spec)

                // Note: steps 10-22 are performed within `hash_message()` below
                let mut phm = [0u8; 64];  // hashers don't all play well with each other
                let (oid, phm_len) = hashing::hash_message(message, ph, &mut phm);

                // Note: step 23 is performed within `sign_internal()` and below.
                // 23: 𝑀 ′ ← BytesToBits(IntegerToBytes(1, 1) ∥ IntegerToBytes(|𝑐𝑡𝑥|, 1) ∥ 𝑐𝑡𝑥 ∥ OID ∥ PH𝑀 )
                // 24: 𝜎 ← ML-DSA.Sign_internal(𝑠𝑘, 𝑀 ′ , 𝑟𝑛𝑑)
                let sig = ml_dsa::sign_internal::<CTEST, K, L, LAMBDA_DIV4, SIG_LEN, SK_LEN, W1_LEN>(
                    BETA, GAMMA1, GAMMA2, OMEGA, TAU, &self, message, ctx, &oid, &phm[0..phm_len], rnd, false
                );

                // 25: return 𝜎
                Ok(sig)
            }


            // Documented in traits.rs
            #[allow(clippy::cast_lossless)]
            fn get_public_key(&self) -> Self::PublicKey {
                ml_dsa::private_to_public_key(&self)
            }
        }


        impl PublicKey {
            /// Verify a FIPS 204 message representative computed by the caller.
            ///
            /// The caller MUST calculate `mu = SHAKE256(tr || 0 || len(ctx) || ctx || M, 64)`
            /// from the actual public key, context and complete message for pure ML-DSA.
            /// This low-level API does not bind an externally supplied digest to a message.
            /// Verification enforces the Subtensor consensus rejection-sampling limits.
            #[must_use]
            pub fn verify_mu(&self, mu: &[u8; 64], sig: &[u8; SIG_LEN]) -> bool {
                ml_dsa::verify_internal_mu::<K, L, LAMBDA_DIV4, SIG_LEN, W1_LEN>(
                    BETA, GAMMA1, GAMMA2, OMEGA, TAU, self, mu, sig,
                )
            }
        }


        impl Verifier for PublicKey {
            type Signature = [u8; SIG_LEN];

            /// # Algorithm 3: `ML-DSA.Verify(pk, 𝑀, 𝜎, ctx)` on page 18.
            /// Verifies a signature 𝜎 for a message 𝑀.
            ///
            /// **Input**:  Implemented on public key struct,
            ///             message `𝑀 ∈ {0, 1}∗`,
            ///             signature `𝜎 ∈ 𝔹^{𝜆/4+ℓ⋅32⋅(1+bitlen(𝛾1−1))+𝜔+𝑘}`,
            ///             context string `ctx` (a byte string of 255 or fewer bytes). <br>
            /// **Output**: Boolean.
            fn verify(&self, message: &[u8], sig: &Self::Signature, ctx: &[u8]) -> bool {
                // 1: if |ctx| > 255 then
                // 2:   return ⊥    ▷ return an error indication if the context string is too long
                // 3: end if
                if ctx.len() > 255 {
                    return false;
                };

                // 4:  (blank line in spec)

                // Note: step 5 is performed within `verify_internal()` and below.
                // 5: 𝑀′ ← BytesToBits(IntegerToBytes(0, 1) ∥ IntegerToBytes(|ctx|, 1) ∥ ctx) ∥ 𝑀
                // 6: return ML-DSA.Verify_internal(pk, 𝑀′, 𝜎)
                ml_dsa::verify_internal::<CTEST, K, L, LAMBDA_DIV4, PK_LEN, SIG_LEN, W1_LEN>(
                    BETA, GAMMA1, GAMMA2, OMEGA, TAU, &self, &message, &sig, ctx, &[], &[], false
                )
            }

            /// # Algorithm 5: `HashML-DSA.Verify(pk, 𝑀, 𝜎, ctx, PH)` on page 21.
            /// Verifies a pre-hash HashML-DSA signature.
            ///
            /// **Input**:  Implemented on public key struct,
            ///             message `𝑀 ∈ {0, 1}∗`,
            ///             signature `𝜎 ∈ 𝔹^{𝜆/4+ℓ⋅32⋅(1+bitlen(𝛾1 −1))+𝜔+𝑘}`,
            ///             context string `ctx` (a byte string of 255 or fewer bytes),
            ///             pre-hash function `PH`. <br>
            /// **Output**: Boolean.
            fn hash_verify(&self, message: &[u8], sig: &Self::Signature, ctx: &[u8], ph: &types::Ph) -> bool {
                // 1: if |ctx| > 255 then
                // 2:   return ⊥    ▷ return an error indication if the context string is too long
                // 3: end if
                if ctx.len() > 255 {
                    return false;
                };

                // 4:  (blank line in spec)

                // Note: steps 5-17 are performed within `hash_message()` below
                let mut phm = [0u8; 64];  // hashers don't all play well with each other
                let (oid, phm_len) = hashing::hash_message(message, ph, &mut phm);

                // Note: step 18 is performed within `verify_internal()` and below.
                // 18: 𝑀′ ← BytesToBits(IntegerToBytes(1, 1) ∥ IntegerToBytes(|ctx|, 1) ∥ ctx ∥ OID ∥ PH𝑀 )
                // 19: return ML-DSA.Verify_internal(𝑝𝑘, 𝑀′ , 𝜎)
                ml_dsa::verify_internal::<CTEST, K, L, LAMBDA_DIV4, PK_LEN, SIG_LEN, W1_LEN>(
                    BETA, GAMMA1, GAMMA2, OMEGA, TAU, &self, &message, &sig, ctx, &oid, &phm[0..phm_len], false
                )
            }
        }


        // ----- SERIALIZATION AND DESERIALIZATION ---

        impl SerDes for PrivateKey {
            type ByteArray = [u8; SK_LEN];


            fn try_from_bytes(sk: Self::ByteArray) -> Result<Self, &'static str> {
                let esk = ml_dsa::expand_private::<K, L, SK_LEN>(ETA, &sk)?;
                Ok(esk)
            }


            fn into_bytes(self) -> Self::ByteArray {
                // Extract the pre-computes
                let PrivateKey {rho, cap_k, tr, s_1_hat_mont: s_hat_1_mont, s_2_hat_mont: s_hat_2_mont, t_0_hat_mont: t_hat_0_mont, ..} = &self;

                // mont->norm each n coeff, of L entries of T, then inverse NTT
                let s_1: [types::R; L] = ntt::inv_ntt(
                    &core::array::from_fn(|l|
                        types::T(core::array::from_fn(|n|
                            helpers::mont_reduce(i64::from(s_hat_1_mont[l].0[n]))))));
                // correct each coeff such that they are centered around 0
                let s_1: [types::R; L] =
                    core::array::from_fn(|l|
                        types::R(core::array::from_fn(|n|
                            if s_1[l].0[n] > (Q / 2) {s_1[l].0[n] - Q} else {s_1[l].0[n]})));

                let s_2: [types::R; K] = ntt::inv_ntt(
                    &core::array::from_fn(|k|
                        types::T(core::array::from_fn(|n|
                            helpers::mont_reduce(i64::from(s_hat_2_mont[k].0[n]))))));
                let s_2: [types::R; K] =
                    core::array::from_fn(|k|
                        types::R(core::array::from_fn(|n|
                            if s_2[k].0[n] > (Q / 2) {s_2[k].0[n] - Q} else {s_2[k].0[n]})));


                let t_0: [types::R; K] = ntt::inv_ntt(
                    &core::array::from_fn(|k|
                        types::T(core::array::from_fn(|n|
                            helpers::mont_reduce(i64::from(t_hat_0_mont[k].0[n]))))));
                let t_0: [types::R; K] =
                    core::array::from_fn(|k|
                        types::R(core::array::from_fn(|n|
                            if t_0[k].0[n] > (Q / 2) {t_0[k].0[n] - Q} else {t_0[k].0[n]})));

                // Encode and return
                encodings::sk_encode::<K, L, SK_LEN>(ETA, rho, cap_k, tr, &s_1, &s_2, &t_0)
            }
        }


        impl SerDes for PublicKey {
            type ByteArray = [u8; PK_LEN];


            fn try_from_bytes(pk: Self::ByteArray) -> Result<Self, &'static str> {
                let epk = ml_dsa::expand_public(&pk)?;
                Ok(epk)

            }


            fn into_bytes(self) -> Self::ByteArray {
                // Extract the pre-computes
                let PublicKey {rho, tr: _tr, t1_d2_hat_mont} = &self;

                // reconstruct t1_d2 then t1
                let t1_d2: [types::R; K] = ntt::inv_ntt(
                    &core::array::from_fn(|k|
                        types::T(core::array::from_fn(|n|
                            helpers::mont_reduce(i64::from(t1_d2_hat_mont[k].0[n]))))));

                let t1: [types::R; K] = core::array::from_fn(|k|
                    types::R(core::array::from_fn(|n|
                        t1_d2[k].0[n] >> D)));

                encodings::pk_encode(rho, &t1)
             }
        }


        #[cfg(test)]
        mod tests {
            use super::*;
            use crate::types::Ph;
            use rand_chacha::rand_core::SeedableRng;

            #[test]
            fn smoke_test() {
                let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(123);
                let message1 = [0u8, 1, 2, 3, 4, 5, 6, 7];
                let message2 = [7u8, 7, 7, 7, 7, 7, 7, 7];

                for _i in 0..32 {
                    let (pk, sk) = try_keygen_with_rng(&mut rng).unwrap();
                    let sig = sk.try_sign_with_rng(&mut rng, &message1, &[]).unwrap();
                    assert!(pk.verify(&message1, &sig, &[]));
                    assert!(!pk.verify(&message2, &sig, &[]));
                    for ph in [Ph::SHA256, Ph::SHA512, Ph::SHAKE128] {
                        let sig = sk.try_hash_sign_with_rng(&mut rng, &message1, &[], &ph).unwrap();
                        let v = pk.hash_verify(&message1, &sig, &[], &ph);
                        assert!(v);
                    }
                    assert_eq!(pk.clone().into_bytes(), sk.get_public_key().into_bytes());
                }

                let (pk, sk) = try_keygen().unwrap();
                let sig = sk.try_sign(&message1, &[]).unwrap();
                assert!(pk.verify(&message1, &sig, &[]));
                assert!(!pk.verify(&message2, &sig, &[]));
                assert!(!pk.verify(&message1, &sig, &[0u8; 257]));
                assert!(sk.try_sign(&message1, &[0u8; 257]).is_err());

                for ph in [Ph::SHA256, Ph::SHA512, Ph::SHAKE128] {
                    let sig = sk.try_hash_sign(&message1, &[], &ph).unwrap();
                    let v = pk.hash_verify(&message1, &sig, &[], &ph);
                    assert!(v);
                }
                assert_eq!(pk.clone().into_bytes(), sk.get_public_key().into_bytes());

                let (pk, sk) = KG::keygen_from_seed(&[0x11u8; 32]);
                let sig = sk.try_sign_with_seed(&[12u8; 32], &message1, &[]).unwrap();
                assert!(pk.verify(&message1, &sig, &[]));
                let sig = sk.try_hash_sign_with_seed(&[34u8; 32], &message1, &[], &Ph::SHA256).unwrap();
                assert!(pk.hash_verify(&message1, &sig, &[], &Ph::SHA256));

                let pk_bytes = pk.into_bytes();
                if pk_bytes.len() == 1312 { assert_eq!(pk_bytes[0], 197) }
                if pk_bytes.len() == 1952 { assert_eq!(pk_bytes[0], 177) }
                if pk_bytes.len() == 2592 { assert_eq!(pk_bytes[0], 16) }

                #[cfg(feature = "dudect")]
                #[allow(deprecated)] {
                assert!(dudect_keygen_sign_with_rng(&mut rng, &[0]).is_ok())
                }
            }
        }


        // ----- SUPPORT FOR DUDECT CONSTANT TIME MEASUREMENTS ---

        /// This function supports the dudect constant-time measurement framework, and
        /// is only exposed with the `dudect` feature is enabled.
        ///
        /// # Errors
        /// Returns an error when the random number generator fails; propagates internal errors.
        #[deprecated = "Function for constant-time testing; do not use elsewhere"]
        #[cfg(feature = "dudect")]
        pub fn dudect_keygen_sign_with_rng(
            rng: &mut impl CryptoRngCore, message: &[u8],
        ) -> Result<[u8; SIG_LEN], &'static str> {
            let (_pk, sk) = ml_dsa::key_gen::<true, K, L, PK_LEN, SK_LEN>(rng, ETA)?;
            let mut rnd = [0u8; 32];
            rng.try_fill_bytes(&mut rnd).map_err(|_| "Random number generator failed")?;
            let sig = ml_dsa::sign_internal::<true, K, L, LAMBDA_DIV4, SIG_LEN, SK_LEN, W1_LEN>(
                BETA, GAMMA1, GAMMA2, OMEGA, TAU, &sk, message, &[1], &[2], &[3], rnd, true
            );
            Ok(sig)
        }

        #[deprecated = "Temporary function to allow application of internal nist vectors; will be removed"]
        /// As of Oct 30 2024, the NIST test vectors are applied to the **internal** functions rather than
        /// the external API.
        ///
        /// The primary difference pertains to the prepending of domain, context, OID and
        /// hash information to the message in the `sign_finish()` and `verify_finish()` functions (follow
        /// the last `nist=true` function argument). This is expected to change such that the full API can
        /// be robustly tested - when this happens, this function will no longer be needed.
        /// # Errors
        /// Propagate errors from the `sign_finish()` function (for failing RNG).
        pub fn _internal_sign(
            sk: &PrivateKey, message: &[u8], ctx: &[u8], rnd: [u8; 32]
        ) -> Result<[u8; SIG_LEN], &'static str> {
            helpers::ensure!(ctx.len() < 256, "_internal_sign: ctx too long");
            let sig = ml_dsa::sign_internal::<CTEST, K, L, LAMBDA_DIV4, SIG_LEN, SK_LEN, W1_LEN>(
                BETA, GAMMA1, GAMMA2, OMEGA, TAU, sk, message, ctx, &[], &[], rnd, true
            );
            Ok(sig)
        }

        #[deprecated = "Temporary function to allow application of internal nist vectors; will be removed"]
        #[must_use]
        /// As of Oct 30 2024, the NIST test vectors are applied to the **internal** functions rather than
        /// the external API.
        ///
        /// The primary difference pertains to the prepending of domain, context, OID and
        /// hash information to the message in the `sign_finish()` and `verify_finish()` functions (follow
        /// the last `nist=true` function argument). This is expected to change such that the full API can
        /// be robustly tested - when this happens, this function will no longer be needed.
        pub fn _internal_verify(pk: &PublicKey, message: &[u8], sig: &[u8; SIG_LEN], ctx: &[u8]) -> bool {
            if ctx.len() > 255 {
                return false;
            };
            ml_dsa::verify_internal::<CTEST, K, L, LAMBDA_DIV4, PK_LEN, SIG_LEN, W1_LEN>(
                BETA, GAMMA1, GAMMA2, OMEGA, TAU, pk, &message, &sig, ctx, &[], &[], true
            )
        }
    };
}

/// # Functionality for the **ML-DSA-44** security parameter set.
///
/// This includes specific sizes for the
/// public key, secret key, and signature along with a number of internal constants. The ML-DSA-44
/// parameter set is claimed to be in security strength category 2.
///
/// **1)** The basic usage is for an originator to start with the [`ml_dsa_44::try_keygen`] function below to
/// generate both [`ml_dsa_44::PublicKey`] and [`ml_dsa_44::PrivateKey`] structs. The resulting
/// [`ml_dsa_44::PrivateKey`] struct implements the [`traits::Signer`] trait which supplies a variety of
/// functions to sign byte-array messages, such as [`traits::Signer::try_sign()`].
///
/// **2)** Both of the `PrivateKey` and `PublicKey` structs implement the [`traits::SerDes`] trait.
/// The originator utilizes the [`traits::SerDes::into_bytes()`] functions to serialize the structs
/// into byte-arrays for storage and/or transmission, similar to the message. Upon retrieval and/or receipt,
/// the remote party utilizes the [`traits::SerDes::try_from_bytes()`] functions to deserialize the
/// byte-arrays into structs.
///
/// **3)** Finally, the remote party uses the [`traits::Verifier::verify()`] function implemented on the
/// [`ml_dsa_44::PublicKey`] struct to verify the message with the `Signature` byte array.
///
/// See the top-level [crate] documentation for example code that implements the above flow.
#[cfg(feature = "ml-dsa-44")]
pub mod ml_dsa_44 {
    const TAU: i32 = 39;
    const LAMBDA: usize = 128;
    const GAMMA1: i32 = 1 << 17;
    const GAMMA2: i32 = (Q - 1) / 88;
    const K: usize = 4;
    const L: usize = 4;
    const ETA: i32 = 2;
    const OMEGA: i32 = 80;
    /// Private (secret) key length in bytes.
    pub const SK_LEN: usize = 2560;
    /// Public key length in bytes.
    pub const PK_LEN: usize = 1312;
    /// Signature length in bytes.
    pub const SIG_LEN: usize = 2420;

    functionality!();
}

/// # Functionality for the **ML-DSA-65** security parameter set.
///
/// This includes specific sizes for the
/// public key, secret key, and signature along with a number of internal constants. The ML-DSA-65
/// parameter set is claimed to be in security strength category 3.
///
/// **1)** The basic usage is for an originator to start with the [`ml_dsa_44::try_keygen`] function below to
/// generate both [`ml_dsa_44::PublicKey`] and [`ml_dsa_44::PrivateKey`] structs. The resulting
/// [`ml_dsa_44::PrivateKey`] struct implements the [`traits::Signer`] trait which supplies a variety of
/// functions to sign byte-array messages, such as [`traits::Signer::try_sign()`].
///
/// **2)** Both of the `PrivateKey` and `PublicKey` structs implement the [`traits::SerDes`] trait
/// The originator utilizes the [`traits::SerDes::into_bytes()`] functions to serialize the structs
/// into byte-arrays for storage and/or transmission, similar to the message. Upon retrieval and/or receipt,
/// the remote party utilizes the [`traits::SerDes::try_from_bytes()`] functions to deserialize the
/// byte-arrays into structs.
///
/// **3)** Finally, the remote party uses the [`traits::Verifier::verify()`] function implemented on the
/// [`ml_dsa_44::PublicKey`] struct to verify the message with the `Signature` byte array.
///
/// See the top-level [crate] documentation for example code that implements the above flow.
#[cfg(feature = "ml-dsa-65")]
pub mod ml_dsa_65 {
    const TAU: i32 = 49;
    const LAMBDA: usize = 192;
    const GAMMA1: i32 = 1 << 19;
    const GAMMA2: i32 = (Q - 1) / 32;
    const K: usize = 6;
    const L: usize = 5;
    const ETA: i32 = 4;
    const OMEGA: i32 = 55;
    /// Private (secret) key length in bytes.
    pub const SK_LEN: usize = 4032;
    /// Public key length in bytes.
    pub const PK_LEN: usize = 1952;
    /// Signature length in bytes.
    pub const SIG_LEN: usize = 3309;

    functionality!();
}

/// # Functionality for the **ML-DSA-87** security parameter set.
///
/// This includes specific sizes for the
/// public key, secret key, and signature along with a number of internal constants. The ML-DSA-87
/// parameter set is claimed to be in security strength category 5.
///
/// **1)** The basic usage is for an originator to start with the [`ml_dsa_44::try_keygen`] function below to
/// generate both [`ml_dsa_44::PublicKey`] and [`ml_dsa_44::PrivateKey`] structs. The resulting
/// [`ml_dsa_44::PrivateKey`] struct implements the [`traits::Signer`] trait which supplies a variety of
/// functions to sign byte-array messages, such as [`traits::Signer::try_sign()`].
///
/// **2)** Both of the `PrivateKey` and `PublicKey` structs implement the [`traits::SerDes`] trait
/// The originator utilizes the [`traits::SerDes::into_bytes()`] functions to serialize the structs
/// into byte-arrays for storage and/or transmission, similar to the message. Upon retrieval and/or receipt,
/// the remote party utilizes the [`traits::SerDes::try_from_bytes()`] functions to deserialize the
/// byte-arrays into structs.
///
/// **3)** Finally, the remote party uses the [`traits::Verifier::verify()`] function implemented on the
/// [`ml_dsa_44::PublicKey`] struct to verify the message with the `Signature` byte array.
///
/// See the top-level [crate] documentation for example code that implements the above flow.
#[cfg(feature = "ml-dsa-87")]
pub mod ml_dsa_87 {
    const TAU: i32 = 60;
    const LAMBDA: usize = 256;
    const GAMMA1: i32 = 1 << 19;
    const GAMMA2: i32 = (Q - 1) / 32;
    const K: usize = 8;
    const L: usize = 7;
    const ETA: i32 = 2;
    const OMEGA: i32 = 75;
    /// Private (secret) key length in bytes.
    pub const SK_LEN: usize = 4896;
    /// Public key length in bytes.
    pub const PK_LEN: usize = 2592;
    /// Signature length in bytes.
    pub const SIG_LEN: usize = 4627;

    functionality!();
}
