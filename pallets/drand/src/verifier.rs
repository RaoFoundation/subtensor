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

//! A collection of verifiers
//!
//!

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
pub type ArkScale<T> = ark_scale::ArkScale<T, USAGE>;

/// construct a message (e.g. signed by drand)
fn message(current_round: RoundNumber, prev_sig: &[u8]) -> Vec<u8> {
    let mut hasher = Sha256::default();
    hasher.update(prev_sig);
    hasher.update(current_round.to_be_bytes());
    hasher.finalize().to_vec()
}

/// something to verify beacon pulses
pub trait Verifier {
    /// verify the given pulse using beacon_config
    fn verify(beacon_config: BeaconConfiguration, pulse: Pulse) -> Result<bool, String>;
}

/// A verifier to check values received from quicknet. It outputs true if valid, false otherwise
///
/// [Quicknet](https://drand.love/blog/quicknet-is-live-on-the-league-of-entropy-mainnet) operates in an unchained mode,
/// so messages contain only the round number. in addition, public keys are in G2 and signatures are
/// in G1
///
/// Values are valid if the pairing equality holds:
///   $e(sig, g_2) == e(msg_on_curve, pk)$
/// where $sig \in \mathbb{G}_1$ is the signature
///       $g_2 \in \mathbb{G}_2$ is a generator
///       $msg_on_curve \in \mathbb{G}_1$ is a hash of the message that drand signed
/// (hash(round_number))       $pk \in \mathbb{G}_2$ is the public key, read from the input public
/// parameters
pub struct QuicknetVerifier;

impl Verifier for QuicknetVerifier {
    fn verify(beacon_config: BeaconConfiguration, pulse: Pulse) -> Result<bool, String> {
        // decode public key (pk)
        let pk =
            ArkScale::<G2AffineOpt>::decode(&mut beacon_config.public_key.into_inner().as_slice())
                .map_err(|e| format!("Failed to decode public key: {e}"))?;

        // decode signature (sigma)
        let signature =
            ArkScale::<G1AffineOpt>::decode(&mut pulse.signature.into_inner().as_slice())
                .map_err(|e| format!("Failed to decode signature: {e}"))?;

        // m = sha256({} || {round})
        let message = message(pulse.round, &[]);
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

        // The BLS pairing check only validates that `signature` is a valid
        // signature for the round number.  It does NOT verify that the
        // `randomness` field was derived from the signature as drand specifies
        // (randomness = sha256(signature)).  Without this second check, an
        // attacker can submit a real signature with a forged `randomness` and
        // bias the pallet's published randomness.
        let expected_randomness: [u8; 32] = Sha256::digest(pulse.signature.into_inner().as_slice()).into();
        if pulse.randomness.as_slice() != expected_randomness.as_slice() {
            return Ok(false);
        }

        Ok(bls12_381::fast_pairing_opt(
            signature.0,
            g2,
            message_on_curve.0,
            pk.0,
        ))
    }
}

/// The unsafe skip verifier is just a pass-through verification, always returns true
pub struct UnsafeSkipVerifier;
impl Verifier for UnsafeSkipVerifier {
    fn verify(_beacon_config: BeaconConfiguration, _pulse: Pulse) -> Result<bool, String> {
        Ok(true)
    }
}
