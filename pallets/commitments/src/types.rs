// This file is part of Substrate.

// Copyright (C) Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: Apache-2.0

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// 	http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use codec::{Codec, Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use frame_support::{
    BoundedVec, CloneNoBound, DebugNoBound, PartialEqNoBound,
    traits::{ConstU32, Get},
};
use scale_info::{
    Path, Type, TypeInfo,
    build::{Fields, Variants},
};
use sp_runtime::traits::{AppendZerosInput, AtLeast32BitUnsigned};
use sp_std::{fmt::Debug, iter::once, prelude::*};
use subtensor_macros::freeze_struct;

/// Represents stored data which can be:
/// - `Raw`: a direct blob up to 128 bytes
/// - `BigRaw`: a larger blob up to 512 bytes
/// - A cryptographic hash (BlakeTwo256, Sha256, Keccak256, ShaThree256)
/// - A timelock-encrypted blob with a reveal round
/// - A reset flag (`ResetBondsFlag`)
///   Can also be `None`.
#[derive(Clone, Default, Eq, PartialEq, Debug, DecodeWithMemTracking, MaxEncodedLen)]
pub enum Data {
    /// No data here.
    #[default]
    None,
    /// The data is stored directly (up to 128 bytes).
    Raw(BoundedVec<u8, ConstU32<128>>),
    /// Only the Blake2 hash of the data is stored. The preimage of the hash may be retrieved
    /// through some hash-lookup service.
    BlakeTwo256([u8; 32]),
    /// Only the SHA2-256 hash of the data is stored. The preimage of the hash may be retrieved
    /// through some hash-lookup service.
    Sha256([u8; 32]),
    /// Only the Keccak-256 hash of the data is stored. The preimage of the hash may be retrieved
    /// through some hash-lookup service.
    Keccak256([u8; 32]),
    /// Only the SHA3-256 hash of the data is stored. The preimage of the hash may be retrieved
    /// through some hash-lookup service.
    ShaThree256([u8; 32]),
    /// A timelock-encrypted commitment with a reveal round.
    TimelockEncrypted {
        encrypted: BoundedVec<u8, ConstU32<MAX_TIMELOCK_COMMITMENT_SIZE_BYTES>>,
        reveal_round: u64,
    },
    /// Flag to trigger bonds reset for subnet
    ResetBondsFlag,
    /// The data is stored directly (up to 512 bytes).
    BigRaw(BoundedVec<u8, ConstU32<MAX_BIGRAW_COMMITMENT_SIZE_BYTES>>),
}

impl Data {
    pub fn is_none(&self) -> bool {
        self == &Data::None
    }

    /// Check if this is a timelock-encrypted commitment.
    pub fn is_timelock_encrypted(&self) -> bool {
        matches!(self, Data::TimelockEncrypted { .. })
    }

    pub fn len_for_rate_limit(&self) -> u64 {
        match self {
            Data::None => 0,
            Data::Raw(bytes) => bytes.len() as u64,
            Data::BlakeTwo256(arr)
            | Data::Sha256(arr)
            | Data::Keccak256(arr)
            | Data::ShaThree256(arr) => arr.len() as u64,
            Data::TimelockEncrypted { encrypted, .. } => encrypted.len() as u64,
            Data::ResetBondsFlag => 0,
            Data::BigRaw(bytes) => bytes.len() as u64,
        }
    }
}

impl Decode for Data {
    fn decode<I: codec::Input>(input: &mut I) -> sp_std::result::Result<Self, codec::Error> {
        let b = input.read_byte()?;
        Ok(match b {
            0 => Data::None,
            n @ 1..=129 => {
                let mut r: BoundedVec<_, _> = vec![0u8; (n as usize).saturating_sub(1)]
                    .try_into()
                    .map_err(|_| codec::Error::from("bound checked in match arm condition; qed"))?;
                input.read(&mut r[..])?;
                Data::Raw(r)
            }
            130 => Data::BlakeTwo256(<[u8; 32]>::decode(input)?),
            131 => Data::Sha256(<[u8; 32]>::decode(input)?),
            132 => Data::Keccak256(<[u8; 32]>::decode(input)?),
            133 => Data::ShaThree256(<[u8; 32]>::decode(input)?),
            134 => {
                let encrypted =
                    BoundedVec::<u8, ConstU32<MAX_TIMELOCK_COMMITMENT_SIZE_BYTES>>::decode(input)?;
                let reveal_round = u64::decode(input)?;
                Data::TimelockEncrypted {
                    encrypted,
                    reveal_round,
                }
            }
            135 => Data::ResetBondsFlag,
            136 => {
                let bigvec =
                    BoundedVec::<u8, ConstU32<MAX_BIGRAW_COMMITMENT_SIZE_BYTES>>::decode(input)?;
                Data::BigRaw(bigvec)
            }
            _ => return Err(codec::Error::from("invalid leading byte")),
        })
    }
}

impl Encode for Data {
    fn encode(&self) -> Vec<u8> {
        match self {
            Data::None => vec![0u8; 1],
            Data::Raw(x) => {
                let l = x.len().min(128) as u8;
                let mut r = vec![l.saturating_add(1)];
                r.extend_from_slice(&x[..]);
                r
            }
            Data::BlakeTwo256(h) => once(130).chain(h.iter().cloned()).collect(),
            Data::Sha256(h) => once(131).chain(h.iter().cloned()).collect(),
            Data::Keccak256(h) => once(132).chain(h.iter().cloned()).collect(),
            Data::ShaThree256(h) => once(133).chain(h.iter().cloned()).collect(),
            Data::TimelockEncrypted {
                encrypted,
                reveal_round,
            } => {
                let mut r = vec![134];
                r.extend_from_slice(&encrypted.encode());
                r.extend_from_slice(&reveal_round.encode());
                r
            }
            Data::ResetBondsFlag => vec![135],
            Data::BigRaw(bigvec) => {
                let mut r = vec![136];
                r.extend_from_slice(&bigvec.encode());
                r
            }
        }
    }
}
impl codec::EncodeLike for Data {}

/// Add a Raw variant with the given index and a fixed sized byte array
macro_rules! data_raw_variants {
    ($variants:ident, $(($index:literal, $size:literal)),* ) => {
		$variants
		$(
			.variant(concat!("Raw", stringify!($size)), |v| v
				.index($index)
				.fields(Fields::unnamed().field(|f| f.ty::<[u8; $size]>()))
			)
		)*
    }
}

impl TypeInfo for Data {
    type Identity = Self;

    fn type_info() -> Type {
        let variants = Variants::new().variant("None", |v| v.index(0));

        // create a variant for all sizes of Raw data from 0-32
        let variants = data_raw_variants!(
            variants,
            (1, 0),
            (2, 1),
            (3, 2),
            (4, 3),
            (5, 4),
            (6, 5),
            (7, 6),
            (8, 7),
            (9, 8),
            (10, 9),
            (11, 10),
            (12, 11),
            (13, 12),
            (14, 13),
            (15, 14),
            (16, 15),
            (17, 16),
            (18, 17),
            (19, 18),
            (20, 19),
            (21, 20),
            (22, 21),
            (23, 22),
            (24, 23),
            (25, 24),
            (26, 25),
            (27, 26),
            (28, 27),
            (29, 28),
            (30, 29),
            (31, 30),
            (32, 31),
            (33, 32),
            (34, 33),
            (35, 34),
            (36, 35),
            (37, 36),
            (38, 37),
            (39, 38),
            (40, 39),
            (41, 40),
            (42, 41),
            (43, 42),
            (44, 43),
            (45, 44),
            (46, 45),
            (47, 46),
            (48, 47),
            (49, 48),
            (50, 49),
            (51, 50),
            (52, 51),
            (53, 52),
            (54, 53),
            (55, 54),
            (56, 55),
            (57, 56),
            (58, 57),
            (59, 58),
            (60, 59),
            (61, 60),
            (62, 61),
            (63, 62),
            (64, 63),
            (65, 64),
            (66, 65),
            (67, 66),
            (68, 67),
            (69, 68),
            (70, 69),
            (71, 70),
            (72, 71),
            (73, 72),
            (74, 73),
            (75, 74),
            (76, 75),
            (77, 76),
            (78, 77),
            (79, 78),
            (80, 79),
            (81, 80),
            (82, 81),
            (83, 82),
            (84, 83),
            (85, 84),
            (86, 85),
            (87, 86),
            (88, 87),
            (89, 88),
            (90, 89),
            (91, 90),
            (92, 91),
            (93, 92),
            (94, 93),
            (95, 94),
            (96, 95),
            (97, 96),
            (98, 97),
            (99, 98),
            (100, 99),
            (101, 100),
            (102, 101),
            (103, 102),
            (104, 103),
            (105, 104),
            (106, 105),
            (107, 106),
            (108, 107),
            (109, 108),
            (110, 109),
            (111, 110),
            (112, 111),
            (113, 112),
            (114, 113),
            (115, 114),
            (116, 115),
            (117, 116),
            (118, 117),
            (119, 118),
            (120, 119),
            (121, 120),
            (122, 121),
            (123, 122),
            (124, 123),
            (125, 124),
            (126, 125),
            (127, 126),
            (128, 127),
            (129, 128)
        );

        let variants = variants
            .variant("BlakeTwo256", |v| {
                v.index(130)
                    .fields(Fields::unnamed().field(|f| f.ty::<[u8; 32]>()))
            })
            .variant("Sha256", |v| {
                v.index(131)
                    .fields(Fields::unnamed().field(|f| f.ty::<[u8; 32]>()))
            })
            .variant("Keccak256", |v| {
                v.index(132)
                    .fields(Fields::unnamed().field(|f| f.ty::<[u8; 32]>()))
            })
            .variant("ShaThree256", |v| {
                v.index(133)
                    .fields(Fields::unnamed().field(|f| f.ty::<[u8; 32]>()))
            })
            .variant("TimelockEncrypted", |v| {
                v.index(134).fields(
                    Fields::named()
                        .field(|f| {
                            f.name("encrypted")
                                .ty::<BoundedVec<u8, ConstU32<MAX_TIMELOCK_COMMITMENT_SIZE_BYTES>>>(
                                )
                        })
                        .field(|f| f.name("reveal_round").ty::<u64>()),
                )
            })
            .variant("ResetBondsFlag", |v| v.index(135))
            .variant("BigRaw", |v| {
                v.index(136).fields(Fields::unnamed().field(|f| {
                    f.ty::<BoundedVec<u8, ConstU32<MAX_BIGRAW_COMMITMENT_SIZE_BYTES>>>()
                }))
            });

        Type::builder()
            .path(Path::new("Data", module_path!()))
            .variant(variants)
    }
}

#[freeze_struct("c236d81fa00063bf")]
#[derive(
    CloneNoBound,
    Encode,
    Decode,
    DecodeWithMemTracking,
    Eq,
    MaxEncodedLen,
    PartialEqNoBound,
    DebugNoBound,
    TypeInfo,
)]
#[codec(mel_bound())]
#[derive(frame_support::DefaultNoBound)]
#[scale_info(skip_type_params(FieldLimit))]
pub struct CommitmentInfo<FieldLimit: Get<u32>> {
    pub fields: BoundedVec<Data, FieldLimit>,
}

/// Maximum size of the serialized timelock commitment in bytes
pub const MAX_TIMELOCK_COMMITMENT_SIZE_BYTES: u32 = 1024;
pub const MAX_BIGRAW_COMMITMENT_SIZE_BYTES: u32 = 512;

/// Contains the decrypted data of a revealed commitment.
#[freeze_struct("bf575857b57f9bef")]
#[derive(Clone, Eq, PartialEq, Encode, Decode, TypeInfo, Debug)]
pub struct RevealedData<Balance, MaxFields: Get<u32>, BlockNumber> {
    pub info: CommitmentInfo<MaxFields>,
    pub revealed_block: BlockNumber,
    pub deposit: Balance,
}

/// Tracks how much “space” each (netuid, who) has used within the current RateLimit block-window.
#[freeze_struct("1f23fb50f96326e4")]
#[derive(Encode, Decode, Default, Clone, PartialEq, Eq, TypeInfo)]
pub struct UsageTracker {
    /// Last epoch block
    pub last_epoch: u64,
    /// Space used
    pub used_space: u64,
}

/// Information concerning the identity of the controller of an account.
///
/// NOTE: This is stored separately primarily to facilitate the addition of extra fields in a
/// backwards compatible way through a specialized `Decode` impl.
#[freeze_struct("8afc71a336adf4f7")]
#[derive(CloneNoBound, DebugNoBound, Encode, Eq, MaxEncodedLen, PartialEqNoBound, TypeInfo)]
#[codec(mel_bound())]
#[scale_info(skip_type_params(MaxFields))]
pub struct Registration<
    Balance: Encode + Decode + MaxEncodedLen + Copy + Clone + Debug + Eq + PartialEq,
    MaxFields: Get<u32>,
    BlockNumber: Codec + Clone + Ord + Eq + AtLeast32BitUnsigned + MaxEncodedLen + Debug,
> {
    /// Amount held on deposit for this information.
    pub deposit: Balance,

    pub block: BlockNumber,

    /// Information on the identity.
    pub info: CommitmentInfo<MaxFields>,
}

// impl<
//         Balance: Encode + Decode + MaxEncodedLen + Copy + Clone + Debug + Eq + PartialEq + Zero + Add,
//         MaxFields: Get<u32>,
//         Block: Codec + Clone + Ord + Eq + AtLeast32BitUnsigned + MaxEncodedLen + Debug,
//     > Registration<Balance, MaxFields, Block>
// {
//     pub(crate) fn total_deposit(&self) -> Balance {
//         self.deposit
//     }
// }

impl<
    Balance: Encode + Decode + MaxEncodedLen + Copy + Clone + Debug + Eq + PartialEq,
    MaxFields: Get<u32>,
    Block: Codec + Clone + Ord + Eq + AtLeast32BitUnsigned + MaxEncodedLen + Debug,
> Decode for Registration<Balance, MaxFields, Block>
{
    fn decode<I: codec::Input>(input: &mut I) -> sp_std::result::Result<Self, codec::Error> {
        let (deposit, block, info) = Decode::decode(&mut AppendZerosInput::new(input))?;
        Ok(Self {
            deposit,
            block,
            info,
        })
    }
}
