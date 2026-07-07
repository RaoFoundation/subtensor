/*
 * Copyright 2024 by Ideal Labs, LLC
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

use ark_ec_05::pairing::Pairing;
use ark_std_05::{Zero, ops::Neg};
use sp_crypto_ec_utils::bls12_381::{
    Bls12_381 as Bls12_381Opt, G1Affine as G1AffineOpt, G2Affine as G2AffineOpt,
};

/// An optimized way to verify Drand pulses from quicket
/// Instead of computing two pairings and comparing them, we instead compute a multi miller loop,
/// and then take the final exponentiation, saving a lot of computational cost.
///
/// This function is also inlined as a way to optimize performance.
///
/// * `signature`:
/// * `q`:
/// * `msg_on_curve`: The message signed by Drand, hashed to G1
/// * `p_pub`: The beacon public key
#[inline]
pub fn fast_pairing_opt(
    signature: G1AffineOpt,
    q: G2AffineOpt,
    r: G1AffineOpt,
    s: G2AffineOpt,
) -> bool {
    let looped = Bls12_381Opt::multi_miller_loop([signature.neg(), r], [q, s]);
    let exp = Bls12_381Opt::final_exponentiation(looped);

    match exp {
        Some(e) => e.is_zero(),
        None => false,
    }
}
