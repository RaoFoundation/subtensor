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

//! BLS verifiers for drand beacon pulses ([`QuicknetVerifier`], [`UnsafeSkipVerifier`]).

use crate::{
    bls12_381,
    types::{BeaconConfiguration, Pulse, RoundNumber},
};
use alloc::{format, string::String, vec::Vec};
use ark_ec::{AffineRepr, hashing::HashToCurve};
use ark_serialize::CanonicalSerialize;
use codec::Decode;
use sha2::{Digest, Sha256};
use sp_crypto_ec_utils::bls12_381::{G1Affine as G1AffineOpt, G2Affine as G2AffineOpt};
use tle::curves::drand::TinyBLS381;
use w3f_bls::engine::EngineBLS;

const USAGE: ark_scale::Usage = ark_scale::WIRE;
/// Arkworks type SCALE wrapper used when decoding beacon keys / signatures from storage.
pub type ArkScale<T> = ark_scale::ArkScale<T, USAGE>;

/// SHA-256 of the round number alone (empty previous signature) — Quicknet unchained message.
pub fn hash_unchained_round_message(current_round: RoundNumber) -> Vec<u8> {
    let mut hasher = Sha256::default();
    hasher.update([]);
    hasher.update(current_round.to_be_bytes());
    hasher.finalize().to_vec()
}

/// Verifies that a [`Pulse`] is a valid signature under a [`BeaconConfiguration`].
pub trait Verifier {
    /// Return `Ok(true)` if `pulse` verifies under `beacon_config`, `Ok(false)` if not,
    /// or `Err` on decode / hash-to-curve failures.
    fn verify(beacon_config: BeaconConfiguration, pulse: Pulse) -> Result<bool, String>;
}

/// Verifier for [Quicknet](https://drand.love/blog/quicknet-is-live-on-the-league-of-entropy-mainnet).
///
/// Quicknet is unchained: the signed message is only the round number. Public keys are in G2
/// and signatures in G1. A pulse is valid when the pairing equality holds:
///
/// `$e(sig, g_2) == e(msg_on_curve, pk)$`
///
/// where `$sig \in G_1$`, `$g_2$` is a G2 generator, `$msg_on_curve$` is hash-to-curve of the
/// round message, and `$pk \in G_2$` comes from [`BeaconConfiguration::public_key`].
pub struct QuicknetVerifier;

impl Verifier for QuicknetVerifier {
    fn verify(beacon_config: BeaconConfiguration, pulse: Pulse) -> Result<bool, String> {
        // decode public key (pk)
        let pk =
            ArkScale::<G2AffineOpt>::decode(&mut beacon_config.public_key.into_inner().as_slice())
                .map_err(|e| format!("Failed to decode public key: {e}"))?;

        // decode signature (sigma)
        let signature = ArkScale::<G1AffineOpt>::decode(&mut pulse.signature.as_slice())
            .map_err(|e| format!("Failed to decode signature: {e}"))?;

        // m = sha256({} || {round})
        let message = hash_unchained_round_message(pulse.round);
        let hasher = <TinyBLS381 as EngineBLS>::hash_to_curve_map();
        // H(m) \in G1
        let message_hash = hasher
            .hash(&message)
            .map_err(|e| format!("Failed to hash message: {e}"))?;

        let mut bytes = Vec::new();
        message_hash
            .serialize_compressed(&mut bytes)
            .map_err(|e| format!("Failed to serialize message hash: {e}"))?;

        let message_on_curve = ArkScale::<G1AffineOpt>::decode(&mut &bytes[..])
            .map_err(|e| format!("Failed to decode message on curve: {e}"))?;

        let g2 = G2AffineOpt::generator();

        Ok(bls12_381::fast_pairing_opt(
            signature.0,
            g2,
            message_on_curve.0,
            pk.0,
        ))
    }
}

/// Test / benchmark verifier that accepts every pulse without a pairing check.
pub struct UnsafeSkipVerifier;
impl Verifier for UnsafeSkipVerifier {
    fn verify(_beacon_config: BeaconConfiguration, _pulse: Pulse) -> Result<bool, String> {
        Ok(true)
    }
}
