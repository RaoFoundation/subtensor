#![allow(clippy::expect_used)]
use crate as pallet_commitments;
use frame_support::{
    derive_impl,
    pallet_prelude::{Get, TypeInfo},
    traits::{ConstU32, ConstU64},
    weights::constants::RocksDbWeight,
};
use sp_core::H256;
use sp_runtime::{
    BuildStorage,
    testing::Header,
    traits::{BlakeTwo256, ConstU16, IdentityLookup},
};
use subtensor_runtime_common::{ConstTao, TaoBalance};

pub type Block = sp_runtime::generic::Block<Header, UncheckedExtrinsic>;
pub type UncheckedExtrinsic =
    sp_runtime::generic::UncheckedExtrinsic<AccountId, RuntimeCall, test_crypto::Signature, ()>;

frame_support::construct_runtime!(
    pub enum Test
    {
        System: frame_system = 1,
        Balances: pallet_balances = 2,
        Commitments: pallet_commitments = 3,
        Drand: pallet_drand = 4,
    }
);

pub type AccountId = u64;

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl frame_system::Config for Test {
    type BaseCallFilter = frame_support::traits::Everything;
    type BlockWeights = ();
    type BlockLength = ();
    type DbWeight = RocksDbWeight;
    type RuntimeOrigin = RuntimeOrigin;
    type RuntimeCall = RuntimeCall;
    type Hash = H256;
    type Hashing = BlakeTwo256;
    type AccountId = u64;
    type Lookup = IdentityLookup<Self::AccountId>;
    type RuntimeEvent = RuntimeEvent;
    type BlockHashCount = ConstU64<250>;
    type Version = ();
    type PalletInfo = PalletInfo;
    type AccountData = pallet_balances::AccountData<TaoBalance>;
    type OnNewAccount = ();
    type OnKilledAccount = ();
    type SystemWeightInfo = ();
    type SS58Prefix = ConstU16<42>;
    type OnSetCode = ();
    type MaxConsumers = ConstU32<16>;
    type Block = Block;
    type Nonce = u32;
}

#[derive_impl(pallet_balances::config_preludes::TestDefaultConfig)]
impl pallet_balances::Config for Test {
    type MaxLocks = ();
    type MaxReserves = ();
    type ReserveIdentifier = [u8; 8];
    type Balance = TaoBalance;
    type RuntimeEvent = RuntimeEvent;
    type DustRemoval = ();
    type ExistentialDeposit = ConstTao<1>;
    type AccountStore = System;
    type WeightInfo = ();
    type FreezeIdentifier = ();
    type MaxFreezes = ();
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TestMaxFields;
impl Get<u32> for TestMaxFields {
    fn get() -> u32 {
        16
    }
}
impl TypeInfo for TestMaxFields {
    type Identity = Self;
    fn type_info() -> scale_info::Type {
        scale_info::Type::builder()
            .path(scale_info::Path::new("TestMaxFields", module_path!()))
            .composite(scale_info::build::Fields::unit())
    }
}

pub struct TestCanCommit;
impl pallet_commitments::CanCommit<u64> for TestCanCommit {
    fn can_commit(_netuid: NetUid, _who: &u64) -> bool {
        true
    }
}

impl pallet_commitments::Config for Test {
    type Currency = Balances;
    type WeightInfo = ();
    type MaxFields = TestMaxFields;
    type CanCommit = TestCanCommit;
    type FieldDeposit = ConstTao<0>;
    type InitialDeposit = ConstTao<0>;
    type TempoInterface = MockTempoInterface;
    type OnMetadataCommitment = ();
}

pub struct MockTempoInterface;
impl pallet_commitments::GetTempoInterface for MockTempoInterface {
    fn get_epoch_index(netuid: NetUid, cur_block: u64) -> u64 {
        let tempo = 360; // TODO: configure SubtensorModule in this mock
        let tempo_plus_one: u64 = tempo.saturating_add(1);
        let netuid_plus_one: u64 = (u16::from(netuid) as u64).saturating_add(1);
        let block_with_offset: u64 = cur_block.saturating_add(netuid_plus_one);

        block_with_offset.checked_div(tempo_plus_one).unwrap_or(0)
    }
}

impl pallet_drand::Config for Test {
    type AuthorityId = test_crypto::TestAuthId;
    type Verifier = pallet_drand::verifier::QuicknetVerifier;
    type UnsignedPriority = ConstU64<{ 1 << 20 }>;
    type HttpFetchTimeout = ConstU64<1_000>;
    type WeightInfo = ();
}

pub mod test_crypto {
    use sp_core::sr25519::{Public as Sr25519Public, Signature as Sr25519Signature};
    use sp_runtime::{
        app_crypto::{app_crypto, sr25519},
        traits::IdentifyAccount,
    };

    pub const KEY_TYPE: sp_runtime::KeyTypeId = sp_runtime::KeyTypeId(*b"test");

    app_crypto!(sr25519, KEY_TYPE);

    pub struct TestAuthId;

    impl frame_system::offchain::AppCrypto<Public, Signature> for TestAuthId {
        type RuntimeAppPublic = Public;
        type GenericSignature = Sr25519Signature;
        type GenericPublic = Sr25519Public;
    }

    impl IdentifyAccount for Public {
        type AccountId = u64;

        fn into_account(self) -> u64 {
            let mut bytes = [0u8; 32];
            bytes.copy_from_slice(self.as_ref());
            u64::from_le_bytes(bytes[..8].try_into().expect("Expected to not panic"))
        }
    }
}

impl frame_system::offchain::SigningTypes for Test {
    type Public = test_crypto::Public;
    type Signature = test_crypto::Signature;
}

impl<LocalCall> frame_system::offchain::CreateTransactionBase<LocalCall> for Test
where
    RuntimeCall: From<LocalCall>,
{
    type Extrinsic = UncheckedExtrinsic;
    type RuntimeCall = RuntimeCall;
}

impl<LocalCall> frame_system::offchain::CreateBare<LocalCall> for Test
where
    RuntimeCall: From<LocalCall>,
{
    fn create_bare(call: Self::RuntimeCall) -> Self::Extrinsic {
        UncheckedExtrinsic::new_bare(call)
    }
}

pub fn new_test_ext() -> sp_io::TestExternalities {
    let t = frame_system::GenesisConfig::<Test>::default()
        .build_storage()
        .expect("Expected to not panic");
    let mut ext = sp_io::TestExternalities::new(t);
    ext.execute_with(|| System::set_block_number(1));
    ext
}

use super::*;
use crate::{EngineBLS, MAX_TIMELOCK_COMMITMENT_SIZE_BYTES, TinyBLS381};
use ark_serialize::CanonicalSerialize;
use frame_support::BoundedVec;
use rand_chacha::{ChaCha20Rng, rand_core::SeedableRng};
use sha2::Digest;
use tle::{ibe::fullident::Identity, stream_ciphers::AESGCMStreamCipherProvider, tlock::tle};

// Drand Quicknet public key and signature for round=1000:
pub const DRAND_QUICKNET_PUBKEY_HEX: &str = "83cf0f2896adee7eb8b5f01fcad3912212c437e0073e911fb90022d3e760183c8c4b450b6\
     a0a6c3ac6a5776a2d1064510d1fec758c921cc22b0e17e63aaf4bcb5ed66304de9cf809b\
     d274ca73bab4af5a6e9c76a4bc09e76eae8991ef5ece45a";
pub const DRAND_QUICKNET_SIG_2000_HEX: &str = "b6cb8f482a0b15d45936a4c4ea08e98a087e71787caee3f4d07a8a9843b1bc5423c6b3c22f446488b3137eaca799c77e"; // round 20000
pub const DRAND_QUICKNET_SIG_HEX: &str = "b44679b9a59af2ec876b1a6b1ad52ea9b1615fc3982b19576350f93447cb1125e342b73a8dd2bacbe47e4b6b63ed5e39";

/// Inserts a Drand pulse for `round` with the given `signature_bytes`.
pub fn insert_drand_pulse(round: u64, signature_bytes: &[u8]) {
    let sig_bounded: BoundedVec<u8, ConstU32<144>> = signature_bytes
        .to_vec()
        .try_into()
        .expect("Signature within 144 bytes");

    let randomness_bounded: BoundedVec<u8, ConstU32<32>> = vec![0u8; 32]
        .try_into()
        .expect("Randomness must be exactly 32 bytes");

    pallet_drand::Pulses::<Test>::insert(
        round,
        pallet_drand::types::Pulse {
            round,
            randomness: randomness_bounded,
            signature: sig_bounded,
        },
    );
}

/// Produces a **real** ciphertext by TLE-encrypting `plaintext` for Drand Quicknet `round`.
///
/// The returned `BoundedVec<u8, ConstU32<MAX_TIMELOCK_COMMITMENT_SIZE_BYTES>>`
/// will decrypt if you pass in the valid signature for the same round.
pub fn produce_ciphertext(
    plaintext: &[u8],
    round: u64,
) -> BoundedVec<u8, ConstU32<MAX_TIMELOCK_COMMITMENT_SIZE_BYTES>> {
    // 1) Deserialize the known Drand Quicknet public key:
    let pub_key_bytes = hex::decode(DRAND_QUICKNET_PUBKEY_HEX).expect("decode pubkey");
    let pub_key =
        <TinyBLS381 as EngineBLS>::PublicKeyGroup::deserialize_compressed(&pub_key_bytes[..])
            .expect("bad pubkey bytes");

    // 2) Prepare the identity for that round
    //    by hashing round.to_be_bytes() with SHA256:
    let msg = {
        let mut hasher = sha2::Sha256::new();
        hasher.update(round.to_be_bytes());
        hasher.finalize().to_vec()
    };
    let identity = Identity::new(b"", vec![msg]);

    // 3) Actually encrypt
    //    (just an example ephemeral secret key & RNG seed)
    let esk = [2u8; 32];
    let rng = ChaCha20Rng::seed_from_u64(0);

    let ct = tle::<TinyBLS381, AESGCMStreamCipherProvider, ChaCha20Rng>(
        pub_key, esk, plaintext, identity, rng,
    )
    .expect("Encryption failed in produce_real_ciphertext");

    // 4) Serialize the ciphertext to BoundedVec
    let mut ct_bytes = Vec::new();
    ct.serialize_compressed(&mut ct_bytes)
        .expect("serialize TLECiphertext");

    ct_bytes.try_into().expect("Ciphertext is within max size")
}
