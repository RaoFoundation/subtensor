#![allow(
    dead_code,
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::unwrap_used
)]

use core::num::NonZeroU64;

use frame_support::dispatch::DispatchResult;
use frame_support::traits::{Contains, Everything, InsideBoth, InstanceFilter};
use frame_support::weights::Weight;
use frame_support::weights::constants::RocksDbWeight;
use frame_support::{PalletId, derive_impl};
use frame_support::{parameter_types, traits::PrivilegeCmp};
use frame_system as system;
use frame_system::{EnsureRoot, limits};
use pallet_subtensor::*;
use pallet_subtensor_proxy as pallet_proxy;
use pallet_subtensor_utility as pallet_utility;
use sp_core::{ConstU64, H256, U256, offchain::KeyTypeId};
use sp_runtime::Perbill;
use sp_runtime::{
    Percent,
    traits::{BlakeTwo256, IdentityLookup},
};
use sp_std::{cell::RefCell, cmp::Ordering, sync::OnceLock};
use sp_tracing::tracing_subscriber;
use subtensor_runtime_common::{AuthorshipInfo, NetUid, TaoBalance};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};
pub type Block = frame_system::mocking::MockBlock<Test>;
pub use api_mocks::MockApi;

// Configure a mock runtime to test the pallet.
frame_support::construct_runtime!(
    pub enum Test
    {
        System: frame_system = 1,
        Balances: pallet_balances = 2,
        Shield: pallet_shield = 3,
        SubtensorModule: pallet_subtensor::pallet = 4,
        Utility: pallet_utility = 5,
        Scheduler: pallet_scheduler = 6,
        Preimage: pallet_preimage = 7,
        Drand: pallet_drand = 8,
        Swap: pallet_subtensor_swap = 9,
        Crowdloan: pallet_crowdloan = 10,
        Proxy: pallet_subtensor_proxy = 11,
        AlphaAssets: pallet_alpha_assets = 12,
    }
);

pub type SubtensorCall = pallet_subtensor::Call<Test>;

pub type SubtensorEvent = pallet_subtensor::Event<Test>;

pub type BalanceCall = pallet_balances::Call<Test>;

pub type TestRuntimeCall = frame_system::Call<Test>;

pub const KEY_TYPE: KeyTypeId = KeyTypeId(*b"test");

pub type AccountId = U256;

// The address format for describing accounts.
pub type Address = AccountId;

// Balance of an account.
pub type Balance = TaoBalance;

// An index to a block.
pub type BlockNumber = u64;

#[derive_impl(pallet_balances::config_preludes::TestDefaultConfig)]
impl pallet_balances::Config for Test {
    type Balance = Balance;
    type RuntimeEvent = RuntimeEvent;
    type DustRemoval = ();
    type ExistentialDeposit = ExistentialDeposit;
    type AccountStore = System;
    type MaxLocks = ();
    type WeightInfo = ();
    type MaxReserves = ();
    type ReserveIdentifier = ();
    type RuntimeHoldReason = ();
    type FreezeIdentifier = ();
    type MaxFreezes = ();
}

impl pallet_shield::Config for Test {
    type AuthorityId = sp_core::sr25519::Public;
    type FindAuthors = ();
    type RuntimeCall = RuntimeCall;
    type ExtrinsicDecryptor = ();
    type WeightInfo = ();
}

pub struct NoNestingCallFilter;

impl Contains<RuntimeCall> for NoNestingCallFilter {
    fn contains(call: &RuntimeCall) -> bool {
        match call {
            RuntimeCall::Utility(inner) => {
                let calls = match inner {
                    pallet_utility::Call::force_batch { calls } => calls,
                    pallet_utility::Call::batch { calls } => calls,
                    pallet_utility::Call::batch_all { calls } => calls,
                    _ => &Vec::new(),
                };

                !calls.iter().any(|call| {
                    matches!(call, RuntimeCall::Utility(inner) if matches!(inner, pallet_utility::Call::force_batch { .. } | pallet_utility::Call::batch_all { .. } | pallet_utility::Call::batch { .. }))
                })
            }
            _ => true,
        }
    }
}

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl system::Config for Test {
    type BaseCallFilter = InsideBoth<Everything, NoNestingCallFilter>;
    type BlockWeights = BlockWeights;
    type BlockLength = ();
    type DbWeight = RocksDbWeight;
    type RuntimeOrigin = RuntimeOrigin;
    type RuntimeCall = RuntimeCall;
    type Hash = H256;
    type Hashing = BlakeTwo256;
    type AccountId = U256;
    type Lookup = IdentityLookup<Self::AccountId>;
    type RuntimeEvent = RuntimeEvent;
    type BlockHashCount = BlockHashCount;
    type Version = ();
    type PalletInfo = PalletInfo;
    type AccountData = pallet_balances::AccountData<TaoBalance>;
    type OnNewAccount = ();
    type OnKilledAccount = ();
    type SystemWeightInfo = ();
    type SS58Prefix = SS58Prefix;
    type OnSetCode = ();
    type MaxConsumers = frame_support::traits::ConstU32<16>;
    type Nonce = u64;
    type Block = Block;
    type DispatchExtension = (
        pallet_subtensor::CheckColdkeySwap<Test>,
        pallet_subtensor::CheckWeights<Test>,
        pallet_subtensor::CheckRateLimits<Test>,
        pallet_subtensor::CheckDelegateTake<Test>,
        pallet_subtensor::CheckServingEndpoints<Test>,
        pallet_subtensor::CheckEvmKeyAssociation<Test>,
    );
}

parameter_types! {
    pub const BlockHashCount: u64 = 250;
    pub const SS58Prefix: u8 = 42;
}

pub const MOCK_BLOCK_BUILDER: u64 = 12345u64;

pub struct MockAuthorshipProvider;

impl AuthorshipInfo<U256> for MockAuthorshipProvider {
    fn author() -> Option<U256> {
        Some(U256::from(MOCK_BLOCK_BUILDER))
    }
}

parameter_types! {
    pub const InitialMinAllowedWeights: u16 = 0;
    pub const InitialEmissionValue: u16 = 0;
    pub BlockWeights: limits::BlockWeights = limits::BlockWeights::with_sensible_defaults(
        Weight::from_parts(2_000_000_000_000, u64::MAX),
        Perbill::from_percent(75),
    );
    pub const ExistentialDeposit: Balance = TaoBalance::new(1);
    pub const TransactionByteFee: Balance = TaoBalance::new(100);
    pub const SDebug:u64 = 1;
    pub const InitialRho: u16 = 30;
    pub const InitialAlphaSigmoidSteepness: i16 = 1000;
    pub const InitialKappa: u16 = 32_767;
    pub const InitialTempo: u16 = 360;
    pub const SelfOwnership: u64 = 2;
    pub const InitialImmunityPeriod: u16 = 2;
    pub const InitialMinAllowedUids: u16 = 2;
    pub const InitialMaxAllowedUids: u16 = 256;
    pub const InitialBondsMovingAverage: u64 = 900_000;
    pub const InitialBondsPenalty:u16 = u16::MAX;
    pub const InitialBondsResetOn: bool = false;
    pub const InitialStakePruningMin: u16 = 0;
    pub const InitialFoundationDistribution: u64 = 0;
    pub const InitialDefaultDelegateTake: u16 = 11_796; // 18%, same as in production
    pub const InitialMinDelegateTake: u16 = 5_898; // 9%;
    pub const InitialDefaultChildKeyTake: u16 = 0 ;// 0 %
    pub const InitialMinChildKeyTake: u16 = 0; // 0 %;
    pub const InitialMaxChildKeyTake: u16 = 11_796; // 18 %;
    pub const InitialWeightsVersionKey: u16 = 0;
    pub const InitialServingRateLimit: u64 = 0; // No limit.
    pub const InitialTxRateLimit: u64 = 0; // Disable rate limit for testing
    pub const InitialTxDelegateTakeRateLimit: u64 = 1; // 1 block take rate limit for testing
    pub const InitialTxChildKeyTakeRateLimit: u64 = 1; // 1 block take rate limit for testing
    pub const InitialBurn: u64 = 0;
    pub const InitialMinBurn: u64 = 500_000;
    pub const InitialMinStake: u64 = 2_000_000;
    pub const InitialMaxBurn: u64 = 1_000_000_000;
    pub const MinBurnUpperBound: TaoBalance = TaoBalance::new(1_000_000_000); // 1 TAO
    pub const MaxBurnLowerBound: TaoBalance = TaoBalance::new(100_000_000); // 0.1 TAO
    pub const MinTempo: u16 = pallet_subtensor::MIN_TEMPO;
    pub const MaxTempo: u16 = pallet_subtensor::MAX_TEMPO;
    pub const MinActivityCutoffFactorMilli: u32 =
        pallet_subtensor::MIN_ACTIVITY_CUTOFF_FACTOR_MILLI;
    pub const MaxActivityCutoffFactorMilli: u32 =
        pallet_subtensor::MAX_ACTIVITY_CUTOFF_FACTOR_MILLI;
    pub const InitialValidatorPruneLen: u64 = 0;
    pub const InitialScalingLawPower: u16 = 50;
    pub const InitialMaxAllowedValidators: u16 = 100;
    pub const InitialIssuance: u64 = 0;
    pub const InitialDifficulty: u64 = 10000;
    pub const InitialActivityCutoff: u16 = 5000;
    pub const InitialAdjustmentInterval: u16 = 100;
    pub const InitialAdjustmentAlpha: u64 = 0; // no weight to previous value.
    pub const InitialMaxRegistrationsPerBlock: u16 = 3;
    pub const InitialTargetRegistrationsPerInterval: u16 = 2;
    pub const InitialPruningScore : u16 = u16::MAX;
    pub const InitialRegistrationRequirement: u16 = u16::MAX; // Top 100%
    pub const InitialMinDifficulty: u64 = 1;
    pub const InitialMaxDifficulty: u64 = u64::MAX;
    pub const InitialRAORecycledForRegistration: u64 = 0;
    pub const InitialNetworkImmunityPeriod: u64 = 1_296_000;
    pub const InitialNetworkMinLockCost: u64 = 100_000_000_000;
    pub const InitialSubnetOwnerCut: u16 = 0; // 0%. 100% of rewards go to validators + miners.
    pub const InitialNetworkLockReductionInterval: u64 = 2; // 2 blocks.
    pub const InitialNetworkRateLimit: u64 = 0;
    pub const InitialKeySwapCost: u64 = 1_000_000_000;
    pub const InitialAlphaHigh: u16 = 58982; // Represents 0.9 as per the production default
    pub const InitialAlphaLow: u16 = 45875; // Represents 0.7 as per the production default
    pub const InitialLiquidAlphaOn: bool = false; // Default value for LiquidAlphaOn
    pub const InitialYuma3On: bool = false; // Default value for Yuma3On
    pub const InitialColdkeySwapAnnouncementDelay: u64 = 50;
    pub const InitialColdkeySwapReannouncementDelay: u64 = 10;
    pub const InitialDissolveNetworkScheduleDuration: u64 =  5 * 24 * 60 * 60 / 12; // Default as 5 days
    pub const InitialTaoWeight: u64 = 0; // 100% global weight.
    pub const InitialEmaPriceHalvingPeriod: u64 = 201_600_u64; // 4 weeks
    pub const InitialStartCallDelay: u64 =  0; // 0 days
    pub const InitialKeySwapOnSubnetCost: u64 = 10_000_000;
    pub const HotkeySwapOnSubnetInterval: u64 = 15; // 15 block, should be bigger than subnet number, then trigger clean up for all subnets
    pub const MaxContributorsPerLeaseToRemove: u32 = 3;
    pub const LeaseDividendsDistributionInterval: u32 = 100;
    pub const MaxImmuneUidsPercentage: Percent = Percent::from_percent(80);
    pub const EvmKeyAssociateRateLimit: u64 = 10;
    pub const SubtensorPalletId: PalletId = PalletId(*b"subtensr");
    pub const BurnAccountId: PalletId = PalletId(*b"burntnsr");
    pub const MaxEpochsPerBlock: u8 = 32;
}

impl pallet_subtensor::Config for Test {
    type RuntimeCall = RuntimeCall;
    type Currency = Balances;
    type InitialIssuance = InitialIssuance;
    type SudoRuntimeCall = TestRuntimeCall;
    type Scheduler = Scheduler;
    type InitialMinAllowedWeights = InitialMinAllowedWeights;
    type InitialEmissionValue = InitialEmissionValue;
    type InitialTempo = InitialTempo;
    type InitialDifficulty = InitialDifficulty;
    type InitialAdjustmentInterval = InitialAdjustmentInterval;
    type InitialAdjustmentAlpha = InitialAdjustmentAlpha;
    type InitialTargetRegistrationsPerInterval = InitialTargetRegistrationsPerInterval;
    type InitialRho = InitialRho;
    type InitialAlphaSigmoidSteepness = InitialAlphaSigmoidSteepness;
    type InitialKappa = InitialKappa;
    type InitialMinAllowedUids = InitialMinAllowedUids;
    type InitialMaxAllowedUids = InitialMaxAllowedUids;
    type InitialValidatorPruneLen = InitialValidatorPruneLen;
    type InitialScalingLawPower = InitialScalingLawPower;
    type InitialImmunityPeriod = InitialImmunityPeriod;
    type InitialActivityCutoff = InitialActivityCutoff;
    type InitialMaxRegistrationsPerBlock = InitialMaxRegistrationsPerBlock;
    type InitialPruningScore = InitialPruningScore;
    type InitialBondsMovingAverage = InitialBondsMovingAverage;
    type InitialBondsPenalty = InitialBondsPenalty;
    type InitialBondsResetOn = InitialBondsResetOn;
    type InitialMaxAllowedValidators = InitialMaxAllowedValidators;
    type InitialDefaultDelegateTake = InitialDefaultDelegateTake;
    type InitialMinDelegateTake = InitialMinDelegateTake;
    type InitialDefaultChildKeyTake = InitialDefaultChildKeyTake;
    type InitialMinChildKeyTake = InitialMinChildKeyTake;
    type InitialMaxChildKeyTake = InitialMaxChildKeyTake;
    type InitialTxChildKeyTakeRateLimit = InitialTxChildKeyTakeRateLimit;
    type InitialWeightsVersionKey = InitialWeightsVersionKey;
    type InitialMaxDifficulty = InitialMaxDifficulty;
    type InitialMinDifficulty = InitialMinDifficulty;
    type InitialServingRateLimit = InitialServingRateLimit;
    type InitialTxRateLimit = InitialTxRateLimit;
    type InitialTxDelegateTakeRateLimit = InitialTxDelegateTakeRateLimit;
    type InitialBurn = InitialBurn;
    type InitialMaxBurn = InitialMaxBurn;
    type InitialMinBurn = InitialMinBurn;
    type InitialMinStake = InitialMinStake;
    type MinBurnUpperBound = MinBurnUpperBound;
    type MaxBurnLowerBound = MaxBurnLowerBound;
    type MinTempo = MinTempo;
    type MaxTempo = MaxTempo;
    type MinActivityCutoffFactorMilli = MinActivityCutoffFactorMilli;
    type MaxActivityCutoffFactorMilli = MaxActivityCutoffFactorMilli;
    type InitialRAORecycledForRegistration = InitialRAORecycledForRegistration;
    type InitialNetworkImmunityPeriod = InitialNetworkImmunityPeriod;
    type InitialNetworkMinLockCost = InitialNetworkMinLockCost;
    type InitialSubnetOwnerCut = InitialSubnetOwnerCut;
    type InitialNetworkLockReductionInterval = InitialNetworkLockReductionInterval;
    type InitialNetworkRateLimit = InitialNetworkRateLimit;
    type KeySwapCost = InitialKeySwapCost;
    type AlphaHigh = InitialAlphaHigh;
    type AlphaLow = InitialAlphaLow;
    type LiquidAlphaOn = InitialLiquidAlphaOn;
    type Yuma3On = InitialYuma3On;
    type Preimages = Preimage;
    type InitialColdkeySwapAnnouncementDelay = InitialColdkeySwapAnnouncementDelay;
    type InitialColdkeySwapReannouncementDelay = InitialColdkeySwapReannouncementDelay;
    type InitialDissolveNetworkScheduleDuration = InitialDissolveNetworkScheduleDuration;
    type InitialTaoWeight = InitialTaoWeight;
    type InitialEmaPriceHalvingPeriod = InitialEmaPriceHalvingPeriod;
    type InitialStartCallDelay = InitialStartCallDelay;
    type SwapInterface = pallet_subtensor_swap::Pallet<Self>;
    type KeySwapOnSubnetCost = InitialKeySwapOnSubnetCost;
    type HotkeySwapOnSubnetInterval = HotkeySwapOnSubnetInterval;
    type ProxyInterface = FakeProxier;
    type LeaseDividendsDistributionInterval = LeaseDividendsDistributionInterval;
    type GetCommitments = ();
    type MaxImmuneUidsPercentage = MaxImmuneUidsPercentage;
    type CommitmentsInterface = CommitmentsI;
    type EvmKeyAssociateRateLimit = EvmKeyAssociateRateLimit;
    type AuthorshipProvider = MockAuthorshipProvider;
    type SubtensorPalletId = SubtensorPalletId;
    type BurnAccountId = BurnAccountId;
    type InitialMaxEpochsPerBlock = MaxEpochsPerBlock;
    type WeightInfo = ();
    type AlphaAssets = AlphaAssets;
}

// Swap-related parameter types
parameter_types! {
    pub const SwapProtocolId: PalletId = PalletId(*b"ten/swap");
    pub const SwapMaxFeeRate: u16 = 10000; // 15.26%
    pub const SwapMinimumLiquidity: u64 = 1_000;
    pub const SwapMinimumReserve: NonZeroU64 = NonZeroU64::new(100).unwrap();
}

impl pallet_subtensor_swap::Config for Test {
    type SubnetInfo = SubtensorModule;
    type BalanceOps = SubtensorModule;
    type ProtocolId = SwapProtocolId;
    type TaoReserve = TaoBalanceReserve<Self>;
    type AlphaReserve = AlphaBalanceReserve<Self>;
    type MaxFeeRate = SwapMaxFeeRate;
    type MinimumLiquidity = SwapMinimumLiquidity;
    type MinimumReserve = SwapMinimumReserve;
    type WeightInfo = ();
}

pub struct OriginPrivilegeCmp;

impl PrivilegeCmp<OriginCaller> for OriginPrivilegeCmp {
    fn cmp_privilege(_left: &OriginCaller, _right: &OriginCaller) -> Option<Ordering> {
        Some(Ordering::Less)
    }
}

pub struct CommitmentsI;
impl CommitmentsInterface for CommitmentsI {
    fn purge_netuid(_netuid: NetUid) {}
}

parameter_types! {
    pub MaximumSchedulerWeight: Weight = Perbill::from_percent(80) *
        BlockWeights::get().max_block;
    pub const MaxScheduledPerBlock: u32 = 50;
    pub const NoPreimagePostponement: Option<u32> = Some(10);
}

impl pallet_scheduler::Config for Test {
    type RuntimeOrigin = RuntimeOrigin;
    type RuntimeEvent = RuntimeEvent;
    type PalletsOrigin = OriginCaller;
    type RuntimeCall = RuntimeCall;
    type MaximumWeight = MaximumSchedulerWeight;
    type ScheduleOrigin = EnsureRoot<AccountId>;
    type MaxScheduledPerBlock = MaxScheduledPerBlock;
    type WeightInfo = pallet_scheduler::weights::SubstrateWeight<Test>;
    type OriginPrivilegeCmp = OriginPrivilegeCmp;
    type Preimages = Preimage;
    type BlockNumberProvider = System;
}

impl pallet_utility::Config for Test {
    type RuntimeCall = RuntimeCall;
    type PalletsOrigin = OriginCaller;
    type WeightInfo = pallet_utility::weights::SubstrateWeight<Test>;
}

parameter_types! {
    pub const PreimageMaxSize: u32 = 4096 * 1024;
    pub const PreimageBaseDeposit: Balance = TaoBalance::new(1);
    pub const PreimageByteDeposit: Balance = TaoBalance::new(1);
}

impl pallet_preimage::Config for Test {
    type WeightInfo = pallet_preimage::weights::SubstrateWeight<Test>;
    type RuntimeEvent = RuntimeEvent;
    type Currency = Balances;
    type ManagerOrigin = EnsureRoot<AccountId>;
    type Consideration = ();
}

thread_local! {
    pub static PROXIES: RefCell<FakeProxier> = const { RefCell::new(FakeProxier(vec![])) };
}

pub struct FakeProxier(pub Vec<(U256, U256)>);

impl ProxyInterface<U256> for FakeProxier {
    fn add_lease_beneficiary_proxy(beneficiary: &AccountId, lease: &AccountId) -> DispatchResult {
        PROXIES.with_borrow_mut(|proxies| {
            proxies.0.push((*beneficiary, *lease));
        });
        Ok(())
    }

    fn remove_lease_beneficiary_proxy(
        beneficiary: &AccountId,
        lease: &AccountId,
    ) -> DispatchResult {
        PROXIES.with_borrow_mut(|proxies| {
            proxies.0.retain(|(b, l)| b != beneficiary && l != lease);
        });
        Ok(())
    }
}

parameter_types! {
    pub const CrowdloanPalletId: PalletId = PalletId(*b"bt/cloan");
    pub const MinimumDeposit: u64 = 50;
    pub const AbsoluteMinimumContribution: u64 = 10;
    pub const MinimumBlockDuration: u64 = 20;
    pub const MaximumBlockDuration: u64 = 100;
    pub const RefundContributorsLimit: u32 = 5;
    pub const MaxContributors: u32 = 10;
}

impl pallet_crowdloan::Config for Test {
    type PalletId = CrowdloanPalletId;
    type Currency = Balances;
    type RuntimeCall = RuntimeCall;
    type WeightInfo = pallet_crowdloan::weights::SubstrateWeight<Test>;
    type Preimages = Preimage;
    type MinimumDeposit = MinimumDeposit;
    type AbsoluteMinimumContribution = AbsoluteMinimumContribution;
    type MinimumBlockDuration = MinimumBlockDuration;
    type MaximumBlockDuration = MaximumBlockDuration;
    type RefundContributorsLimit = RefundContributorsLimit;
    type MaxContributors = MaxContributors;
}

// Proxy Pallet config
parameter_types! {
    // Set as 1 for testing purposes
    pub const ProxyDepositBase: Balance = TaoBalance::new(1);
    // Set as 1 for testing purposes
    pub const ProxyDepositFactor: Balance = TaoBalance::new(1);
    // Set as 20 for testing purposes
    pub const MaxProxies: u32 = 20; // max num proxies per acct
    // Set as 15 for testing purposes
    pub const MaxPending: u32 = 15; // max blocks pending ~15min
    // Set as 1 for testing purposes
    pub const AnnouncementDepositBase: Balance =  TaoBalance::new(1);
    // Set as 1 for testing purposes
    pub const AnnouncementDepositFactor: Balance = TaoBalance::new(1);
}

impl pallet_proxy::Config for Test {
    type RuntimeCall = RuntimeCall;
    type Currency = Balances;
    type ProxyType = subtensor_runtime_common::ProxyType;
    type ProxyDepositBase = ProxyDepositBase;
    type ProxyDepositFactor = ProxyDepositFactor;
    type MaxProxies = MaxProxies;
    type WeightInfo = pallet_proxy::weights::SubstrateWeight<Test>;
    type MaxPending = MaxPending;
    type CallHasher = BlakeTwo256;
    type AnnouncementDepositBase = AnnouncementDepositBase;
    type AnnouncementDepositFactor = AnnouncementDepositFactor;
    type BlockNumberProvider = System;
}

impl InstanceFilter<RuntimeCall> for subtensor_runtime_common::ProxyType {
    fn filter(&self, _c: &RuntimeCall) -> bool {
        // In tests, allow all proxy types to pass through
        true
    }
    fn is_superset(&self, o: &Self) -> bool {
        match (self, o) {
            (x, y) if x == y => true,
            (subtensor_runtime_common::ProxyType::Any, _) => true,
            _ => false,
        }
    }
}

impl pallet_alpha_assets::Config for Test {}

mod test_crypto {
    use super::KEY_TYPE;
    use sp_core::{
        U256,
        sr25519::{Public as Sr25519Public, Signature as Sr25519Signature},
    };
    use sp_runtime::{
        app_crypto::{app_crypto, sr25519},
        traits::IdentifyAccount,
    };

    app_crypto!(sr25519, KEY_TYPE);

    pub struct TestAuthId;

    impl frame_system::offchain::AppCrypto<Public, Signature> for TestAuthId {
        type RuntimeAppPublic = Public;
        type GenericSignature = Sr25519Signature;
        type GenericPublic = Sr25519Public;
    }

    impl IdentifyAccount for Public {
        type AccountId = U256;

        fn into_account(self) -> U256 {
            let mut bytes = [0u8; 32];
            bytes.copy_from_slice(self.as_ref());
            U256::from_big_endian(&bytes)
        }
    }
}

pub type TestAuthId = test_crypto::TestAuthId;

impl pallet_drand::Config for Test {
    type AuthorityId = TestAuthId;
    type Verifier = pallet_drand::verifier::QuicknetVerifier;
    type UnsignedPriority = ConstU64<{ 1 << 20 }>;
    type HttpFetchTimeout = ConstU64<1_000>;
    type WeightInfo = ();
}

impl frame_system::offchain::SigningTypes for Test {
    type Public = test_crypto::Public;
    type Signature = test_crypto::Signature;
}

pub type UncheckedExtrinsic = sp_runtime::testing::TestXt<RuntimeCall, ()>;

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

static TEST_LOGS_INIT: OnceLock<()> = OnceLock::new();

pub fn init_logs_for_tests() {
    if TEST_LOGS_INIT.get().is_some() {
        return;
    }

    // RUST_LOG (full syntax) or "off" if unset
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("off"));

    // Bridge log -> tracing (ok if already set)
    let _ = tracing_log::LogTracer::init();

    // Simple formatter
    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_target(true)
        .with_level(true)
        .without_time();

    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(fmt_layer)
        .try_init();

    let _ = TEST_LOGS_INIT.set(());
}

#[allow(dead_code)]
pub fn add_balance_to_coldkey_account(coldkey: &U256, tao: TaoBalance) {
    let credit = SubtensorModule::mint_tao(tao);
    let _ = SubtensorModule::spend_tao(coldkey, credit, tao).unwrap();
}

mod api_mocks {
    use codec::Compact;
    use pallet_subtensor::rpc_info::delegate_info::DelegateInfo;
    use pallet_subtensor::rpc_info::stake_info::StakeInfo;
    use pallet_subtensor_swap_runtime_api::{SimSwapResult, SubnetPrice, SwapRuntimeApi};
    use sp_runtime::AccountId32;
    use subtensor_custom_rpc_runtime_api::{DelegateInfoRuntimeApi, StakeInfoRuntimeApi};
    use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance};

    use super::Block;

    pub struct MockApi;

    sp_api::mock_impl_runtime_apis! {
        impl DelegateInfoRuntimeApi<Block> for MockApi {
            fn get_delegates() -> Vec<DelegateInfo<AccountId32>> { Vec::new() }
            fn get_delegate(_delegate_account: AccountId32) -> Option<DelegateInfo<AccountId32>> { None }
            fn get_delegated(
                _delegatee_account: AccountId32,
            ) -> Vec<(DelegateInfo<AccountId32>, (Compact<NetUid>, Compact<AlphaBalance>))> {
                Vec::new()
            }
        }

        impl StakeInfoRuntimeApi<Block> for MockApi {
            fn get_stake_info_for_coldkey(_coldkey_account: AccountId32) -> Vec<StakeInfo<AccountId32>> {
                Vec::new()
            }
            fn get_stake_info_for_coldkeys(
                _coldkey_accounts: Vec<AccountId32>,
            ) -> Vec<(AccountId32, Vec<StakeInfo<AccountId32>>)> {
                Vec::new()
            }
            fn get_stake_info_for_hotkey_coldkey_netuid(
                _hotkey_account: AccountId32,
                _coldkey_account: AccountId32,
                _netuid: NetUid,
            ) -> Option<StakeInfo<AccountId32>> {
                None
            }
            fn get_stake_fee(
                _origin: Option<(AccountId32, NetUid)>,
                _origin_coldkey_account: AccountId32,
                _destination: Option<(AccountId32, NetUid)>,
                _destination_coldkey_account: AccountId32,
                _amount: u64,
            ) -> u64 {
                0
            }
        }

        impl SwapRuntimeApi<Block> for MockApi {
            fn current_alpha_price(_netuid: NetUid) -> u64 { 0 }
            fn current_alpha_price_all() -> Vec<SubnetPrice> { Vec::new() }
            fn sim_swap_tao_for_alpha(_netuid: NetUid, _tao: TaoBalance) -> SimSwapResult {
                SimSwapResult {
                    tao_amount: 0u64.into(),
                    alpha_amount: 0u64.into(),
                    tao_fee: 0u64.into(),
                    alpha_fee: 0u64.into(),
                    tao_slippage: 0u64.into(),
                    alpha_slippage: 0u64.into(),
                }
            }
            fn sim_swap_alpha_for_tao(_netuid: NetUid, _alpha: AlphaBalance) -> SimSwapResult {
                SimSwapResult {
                    tao_amount: 0u64.into(),
                    alpha_amount: 0u64.into(),
                    tao_fee: 0u64.into(),
                    alpha_fee: 0u64.into(),
                    tao_slippage: 0u64.into(),
                    alpha_slippage: 0u64.into(),
                }
            }
        }
    }
}
