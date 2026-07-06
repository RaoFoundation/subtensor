use crate as pallet_drand_bridge;
use crate::verifier::*;
use crate::*;
use frame_support::{
    derive_impl, parameter_types,
    traits::{ConstU16, ConstU64},
};
use sp_core::{H256, sr25519::Signature};
use sp_keystore::{KeystoreExt, testing::MemoryKeystore};
use sp_runtime::{
    BuildStorage,
    testing::TestXt,
    traits::{BlakeTwo256, IdentityLookup, Verify},
};

type Block = frame_system::mocking::MockBlock<Test>;

// Configure a mock runtime to test the pallet.
frame_support::construct_runtime!(
    pub enum Test
    {
        System: frame_system = 1,
        Drand: pallet_drand_bridge = 2,
    }
);

#[derive_impl(frame_system::config_preludes::TestDefaultConfig as frame_system::DefaultConfig)]
impl frame_system::Config for Test {
    type BaseCallFilter = frame_support::traits::Everything;
    type BlockWeights = ();
    type BlockLength = ();
    type DbWeight = ();
    type RuntimeOrigin = RuntimeOrigin;
    type RuntimeCall = RuntimeCall;
    type Nonce = u64;
    type Hash = H256;
    type Hashing = BlakeTwo256;
    type AccountId = sp_core::sr25519::Public;
    type Lookup = IdentityLookup<Self::AccountId>;
    type Block = Block;
    type RuntimeEvent = RuntimeEvent;
    type BlockHashCount = ConstU64<250>;
    type Version = ();
    type PalletInfo = PalletInfo;
    type AccountData = ();
    type OnNewAccount = ();
    type OnKilledAccount = ();
    type SystemWeightInfo = ();
    type SS58Prefix = ConstU16<42>;
    type OnSetCode = ();
    type MaxConsumers = frame_support::traits::ConstU32<16>;
}

type Extrinsic = TestXt<RuntimeCall, ()>;

impl frame_system::offchain::SigningTypes for Test {
    type Public = <Signature as Verify>::Signer;
    type Signature = Signature;
}

impl<LocalCall> frame_system::offchain::CreateTransactionBase<LocalCall> for Test
where
    RuntimeCall: From<LocalCall>,
{
    type RuntimeCall = RuntimeCall;
    type Extrinsic = Extrinsic;
}

impl<LocalCall> frame_system::offchain::CreateBare<LocalCall> for Test
where
    RuntimeCall: From<LocalCall>,
{
    fn create_bare(call: RuntimeCall) -> Self::Extrinsic {
        Extrinsic::new_bare(call)
    }
}

parameter_types! {
    pub const UnsignedPriority: u64 = 1 << 20;
}

impl pallet_drand_bridge::Config for Test {
    type AuthorityId = crypto::TestAuthId;
    type Verifier = QuicknetVerifier;
    type UnsignedPriority = UnsignedPriority;
    type HttpFetchTimeout = ConstU64<1_000>;
    type WeightInfo = ();
}

// Build genesis storage according to the mock runtime.
pub fn new_test_ext() -> sp_io::TestExternalities {
    let t = frame_system::GenesisConfig::<Test>::default()
        .build_storage()
        .unwrap();

    let mut ext = sp_io::TestExternalities::new(t);
    let keystore = MemoryKeystore::new();
    ext.register_extension(KeystoreExt::new(keystore.clone()));
    sp_keystore::Keystore::sr25519_generate_new(
        &keystore,
        pallet_drand_bridge::KEY_TYPE,
        Some("//Alice"),
    )
    .expect("Creating key with account Alice should succeed.");
    ext
}
