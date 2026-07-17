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

use alloc::{string::String, vec::Vec};
use codec::{Decode, DecodeWithMemTracking, Encode};
use frame_support::pallet_prelude::*;
use serde::{Deserialize, Serialize};
use subtensor_macros::freeze_struct;

/// Represents an opaque public key used in drand's quicknet
pub type OpaquePublicKey = BoundedVec<u8, ConstU32<96>>;

/// an opaque hash type
pub type BoundedHash = BoundedVec<u8, ConstU32<32>>;
/// the round number to track rounds of the beacon
pub type RoundNumber = u64;

/// the expected response body from the drand api endpoint `api.drand.sh/{chainId}/info`
#[freeze_struct("f9e09b3273fe00cd")]
#[derive(Debug, Decode, Default, PartialEq, Encode, Serialize, Deserialize, TypeInfo, Clone)]
pub struct BeaconInfoResponse {
    #[serde(with = "hex::serde")]
    pub public_key: Vec<u8>,
    pub period: u32,
    pub genesis_time: u32,
    #[serde(with = "hex::serde")]
    pub hash: Vec<u8>,
    #[serde(with = "hex::serde", rename = "groupHash")]
    pub group_hash: Vec<u8>,
    #[serde(rename = "schemeID")]
    pub scheme_id: String,
    pub metadata: MetadataInfoResponse,
}

/// metadata associated with the drand info response
#[freeze_struct("91c762d05dbf1d21")]
#[derive(Debug, Decode, Default, PartialEq, Encode, Serialize, Deserialize, TypeInfo, Clone)]
pub struct MetadataInfoResponse {
    #[serde(rename = "beaconID")]
    beacon_id: String,
}

impl BeaconInfoResponse {
    /// the default configuration fetches from quicknet
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

/// a pulse from the drand beacon
/// the expected response body from the drand api endpoint `api.drand.sh/{chainId}/public/latest`
#[freeze_struct("a3fed2c99a0638bf")]
#[derive(Debug, Decode, Default, PartialEq, Encode, Serialize, Deserialize)]
pub struct DrandResponseBody {
    /// the randomness round number
    pub round: RoundNumber,
    /// the sha256 hash of the signature
    // TODO: use Hash (https://github.com/ideal-lab5/pallet-drand/issues/2)
    #[serde(with = "hex::serde")]
    pub randomness: Vec<u8>,
    /// BLS sig for the current round
    // TODO: use Signature (https://github.com/ideal-lab5/pallet-drand/issues/2)
    #[serde(with = "hex::serde")]
    pub signature: Vec<u8>,
}

impl DrandResponseBody {
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
/// A drand chain configuration
#[freeze_struct("e839cb287e55b4f5")]
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
    pub public_key: OpaquePublicKey,
    pub period: u32,
    pub genesis_time: u32,
    pub hash: BoundedHash,
    pub group_hash: BoundedHash,
    pub scheme_id: BoundedHash,
    pub metadata: Metadata,
}

/// Payload used by to hold the beacon
/// config required to submit a transaction.
#[freeze_struct("aa582bfb5fcb7d4f")]
#[derive(Encode, Decode, DecodeWithMemTracking, Debug, Clone, PartialEq, scale_info::TypeInfo)]
pub struct BeaconConfigurationPayload<Public, BlockNumber> {
    pub block_number: BlockNumber,
    pub config: BeaconConfiguration,
    pub public: Public,
}

/// metadata for the drand beacon configuration
#[freeze_struct("e4cfd191c043f56f")]
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
    pub beacon_id: BoundedHash,
}

/// A pulse from the drand beacon
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
    /// the randomness round number
    pub round: RoundNumber,
    /// the sha256 hash of the signature
    // TODO: use Hash (https://github.com/ideal-lab5/pallet-drand/issues/2)
    pub randomness: BoundedVec<u8, ConstU32<32>>,
    /// BLS sig for the current round
    // TODO: use Signature (https://github.com/ideal-lab5/pallet-drand/issues/2)
    // maybe add the sig size as a generic?
    pub signature: BoundedVec<u8, ConstU32<144>>,
}

/// Payload used by to hold the pulse
/// data required to submit a transaction.
#[freeze_struct("7bc8788d055c45e3")]
#[derive(Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, Debug, TypeInfo)]
pub struct PulsesPayload<Public, BlockNumber> {
    pub block_number: BlockNumber,
    pub pulses: Vec<Pulse>,
    pub public: Public,
}
