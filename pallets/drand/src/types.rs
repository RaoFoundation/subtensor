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

//! SCALE / JSON types for drand Quicknet pulses and beacon configuration.

use alloc::{string::String, vec::Vec};
use codec::{Decode, DecodeWithMemTracking, Encode};
use frame_support::pallet_prelude::*;
use serde::{Deserialize, Serialize};
use subtensor_macros::freeze_struct;

/// Opaque Quicknet G2 public key bytes (96 bytes when Tiny BLS381).
pub type OpaquePublicKey = BoundedVec<u8, ConstU32<96>>;

/// Opaque 32-byte hash (chain hash, group hash, scheme id encoding, etc.).
pub type BoundedHash = BoundedVec<u8, ConstU32<32>>;
/// Drand beacon round index (increments every `period` seconds on Quicknet).
pub type RoundNumber = u64;

/// JSON body from `GET …/{chainId}/info` before conversion to [`BeaconConfiguration`].
#[freeze_struct("f9e2d735dd9fb3b3")]
#[derive(Debug, Decode, Default, PartialEq, Encode, Serialize, Deserialize, TypeInfo, Clone)]
pub struct BeaconInfoResponse {
    /// Hex-encoded beacon public key from the HTTP API.
    #[serde(with = "hex::serde")]
    pub public_key: Vec<u8>,
    /// Seconds between rounds on this chain.
    pub period: u32,
    /// Unix timestamp of round 1 genesis.
    pub genesis_time: u32,
    /// Hex-encoded chain hash.
    #[serde(with = "hex::serde")]
    pub hash: Vec<u8>,
    /// Hex-encoded group hash.
    #[serde(with = "hex::serde", rename = "groupHash")]
    pub group_hash: Vec<u8>,
    /// Scheme identifier string (e.g. `bls-unchained-g1-rfc9380`).
    #[serde(rename = "schemeID")]
    pub scheme_id: String,
    /// Nested beacon metadata from the info response.
    pub metadata: MetadataInfoResponse,
}

/// Nested `metadata` object inside a drand `/info` JSON response.
#[freeze_struct("199c70163a6d97a8")]
#[derive(Debug, Decode, Default, PartialEq, Encode, Serialize, Deserialize, TypeInfo, Clone)]
pub struct MetadataInfoResponse {
    /// Beacon id string (Quicknet uses `quicknet`).
    #[serde(rename = "beaconID")]
    beacon_id: String,
}

impl BeaconInfoResponse {
    /// Convert unbounded HTTP `/info` fields into on-chain [`BeaconConfiguration`] bounds.
    pub fn try_into_beacon_config(&self) -> Result<BeaconConfiguration, String> {
        let bounded_pubkey = OpaquePublicKey::try_from(self.public_key.clone())
            .map_err(|_| "Failed to convert public_key")?;
        let bounded_hash =
            BoundedHash::try_from(self.hash.clone()).map_err(|_| "Failed to convert hash")?;
        let bounded_group_hash = BoundedHash::try_from(self.group_hash.clone())
            .map_err(|_| "Failed to convert group_hash")?;
        let bounded_scheme_id = BoundedHash::try_from(self.scheme_id.as_bytes().to_vec().clone())
            .map_err(|_| "Failed to convert scheme_id")?;
        let bounded_beacon_id =
            BoundedHash::try_from(self.metadata.beacon_id.as_bytes().to_vec().clone())
                .map_err(|_| "Failed to convert beacon_id")?;

        Ok(BeaconConfiguration {
            public_key: bounded_pubkey,
            period: self.period,
            genesis_time: self.genesis_time,
            hash: bounded_hash,
            group_hash: bounded_group_hash,
            scheme_id: bounded_scheme_id,
            metadata: Metadata {
                beacon_id: bounded_beacon_id,
            },
        })
    }
}

/// JSON body from `GET …/{chainId}/public/{round|latest}` before conversion to [`Pulse`].
#[freeze_struct("e4eceee3fd13178b")]
#[derive(Debug, Decode, Default, PartialEq, Encode, Serialize, Deserialize)]
pub struct DrandResponseBody {
    /// Round index for this pulse.
    pub round: RoundNumber,
    /// Hex-encoded sha256 of the BLS signature (API `randomness` field).
    // TODO: use Hash (https://github.com/ideal-lab5/pallet-drand/issues/2)
    #[serde(with = "hex::serde")]
    pub randomness: Vec<u8>,
    /// Hex-encoded BLS signature for this round.
    // TODO: use Signature (https://github.com/ideal-lab5/pallet-drand/issues/2)
    #[serde(with = "hex::serde")]
    pub signature: Vec<u8>,
}

impl DrandResponseBody {
    /// Convert unbounded HTTP pulse fields into an on-chain [`Pulse`].
    pub fn try_into_pulse(&self) -> Result<Pulse, String> {
        // TODO:  update these bounded vecs
        let bounded_randomness = BoundedVec::<u8, ConstU32<32>>::try_from(self.randomness.clone())
            .map_err(|_| "Failed to convert randomness")?;
        // TODO: why is the sig size so big?
        let bounded_signature = BoundedVec::<u8, ConstU32<144>>::try_from(self.signature.clone())
            .map_err(|_| "Failed to convert signature")?;

        Ok(Pulse {
            round: self.round,
            randomness: bounded_randomness,
            signature: bounded_signature,
        })
    }
}

/// On-chain drand chain parameters stored in [`crate::BeaconConfig`].
#[freeze_struct("cecf61bb24ece161")]
#[derive(
    Clone,
    Debug,
    Decode,
    DecodeWithMemTracking,
    Default,
    PartialEq,
    Encode,
    Serialize,
    Deserialize,
    MaxEncodedLen,
    TypeInfo,
)]
pub struct BeaconConfiguration {
    /// BLS public key used to verify pulses.
    pub public_key: OpaquePublicKey,
    /// Seconds between consecutive rounds.
    pub period: u32,
    /// Unix genesis time of round 1.
    pub genesis_time: u32,
    /// Chain hash identifying this beacon.
    pub hash: BoundedHash,
    /// Group hash from the beacon info.
    pub group_hash: BoundedHash,
    /// Scheme id bytes (e.g. unchained G1 RFC9380).
    pub scheme_id: BoundedHash,
    /// Beacon metadata (id string as bytes).
    pub metadata: Metadata,
}

/// Unsigned-tx payload carrying a new [`BeaconConfiguration`] plus signer metadata.
#[freeze_struct("381d5ee5cfb1db23")]
#[derive(Encode, Decode, DecodeWithMemTracking, Debug, Clone, PartialEq, scale_info::TypeInfo)]
pub struct BeaconConfigurationPayload<Public, BlockNumber> {
    /// Block number observed by the offchain worker when building the payload.
    pub block_number: BlockNumber,
    /// Beacon parameters to write into storage.
    pub config: BeaconConfiguration,
    /// Local authority public key that signed this payload.
    pub public: Public,
}

/// On-chain beacon metadata nested in [`BeaconConfiguration`].
#[freeze_struct("52e3179192cb40fd")]
#[derive(
    Clone,
    Debug,
    Decode,
    DecodeWithMemTracking,
    Default,
    PartialEq,
    Encode,
    Serialize,
    Deserialize,
    MaxEncodedLen,
    TypeInfo,
)]
pub struct Metadata {
    /// Beacon id bytes (Quicknet: ASCII `quicknet`).
    pub beacon_id: BoundedHash,
}

/// One verified (or pending) Quicknet pulse stored under [`crate::Pulses`].
#[freeze_struct("3836b1f8846739fc")]
#[derive(
    Clone,
    Debug,
    Decode,
    DecodeWithMemTracking,
    Default,
    PartialEq,
    Encode,
    Serialize,
    Deserialize,
    MaxEncodedLen,
    TypeInfo,
    Eq,
)]
pub struct Pulse {
    /// Round index for this pulse.
    pub round: RoundNumber,
    /// Sha256 of the BLS signature (32 bytes).
    // TODO: use Hash (https://github.com/ideal-lab5/pallet-drand/issues/2)
    pub randomness: BoundedVec<u8, ConstU32<32>>,
    /// BLS signature bytes for this round.
    // TODO: use Signature (https://github.com/ideal-lab5/pallet-drand/issues/2)
    // maybe add the sig size as a generic?
    pub signature: BoundedVec<u8, ConstU32<144>>,
}

/// Unsigned-tx payload of one or more [`Pulse`]s plus signer metadata.
#[freeze_struct("ce91cf9cce9f7d48")]
#[derive(Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, RuntimeDebug, TypeInfo)]
pub struct PulsesPayload<Public, BlockNumber> {
    /// Block number observed by the offchain worker when building the payload.
    pub block_number: BlockNumber,
    /// Pulses to verify and append (runtime currently submits one per extrinsic).
    pub pulses: Vec<Pulse>,
    /// Local authority public key that signed this payload.
    pub public: Public,
}
