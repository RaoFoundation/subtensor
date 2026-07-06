#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used)]

use core::num::NonZeroU64;

use crate::TransactionFeeHandler;
use frame_support::pallet_prelude::Zero;
use frame_support::{
    PalletId, assert_ok, derive_impl, parameter_types,
    traits::{Everything, Hooks, PrivilegeCmp},
    weights::IdentityFee,
};
use frame_system::{self as system, EnsureRoot, RawOrigin, limits};
pub use pallet_subtensor::*;
pub use sp_core::U256;
use sp_core::{ConstU64, H256};
use sp_runtime::{
    BuildStorage, KeyTypeId, Perbill, Percent,
    testing::TestXt,
    traits::{BlakeTwo256, ConstU32, IdentityLookup, One},
};
use sp_std::cmp::Ordering;
use sp_weights::Weight;
pub use subtensor_runtime_common::{
    AlphaBalance, AuthorshipInfo, ConstTao, NetUid, TaoBalance, Token,
};
use subtensor_swap_interface::{Order, SwapHandler};

use crate::SubtensorTxFeeHandler;
use pallet_transaction_payment::{ConstFeeMultiplier, Multiplier};

pub const TAO: u64 = 1_000_000_000;

type Block = frame_system::mocking::MockBlock<Test>;

// Configure a mock runtime to test the pallet.
frame_support::construct_runtime!(
    pub enum Test {
        System: frame_system = 1,
        Balances: pallet_balances = 2,
        AlphaAssets: pallet_alpha_assets = 3,
        SubtensorModule: pallet_subtensor::{Pallet, Call, Storage, Event<T>, Error<T>} = 4,
        Scheduler: pallet_scheduler::{Pallet, Call, Storage, Event<T>} = 5,
        Drand: pallet_drand::{Pallet, Call, Storage, Event<T>} = 6,
        Grandpa: pallet_grandpa = 7,
        EVMChainId: pallet_evm_chain_id = 8,
        Swap: pallet_subtensor_swap::{Pallet, Call, Storage, Event<T>} = 9,
        Preimage: pallet_preimage::{Pallet, Call, Storage, Event<T>} = 10,
        Crowdloan: pallet_crowdloan::{Pallet, Call, Storage, Event<T>} = 11,
        TransactionPayment: pallet_transaction_payment = 12,
    }
);

#[allow(dead_code)]
pub type SubtensorCall = pallet_subtensor::Call<Test>;

#[allow(dead_code)]
pub type SubtensorEvent = pallet_subtensor::Event<Test>;

#[allow(dead_code)]
pub type BalanceCall = pallet_balances::Call<Test>;

parameter_types! {
    pub const BlockHashCount: u64 = 250;
    pub const SS58Prefix: u8 = 42;
}

#[allow(dead_code)]
pub type AccountId = U256;

// The address format for describing accounts.
#[allow(dead_code)]
pub type Address = AccountId;

// Balance of an account.
#[allow(dead_code)]
pub type Balance = TaoBalance;

// An index to a block.
#[allow(dead_code)]
pub type BlockNumber = u64;

pub type TestAuthId = test_crypto::TestAuthId;

pub type TransactionExtensions = (
    frame_system::CheckNonZeroSender<Test>,
    frame_system::CheckWeight<Test>,
    pallet_transaction_payment::ChargeTransactionPayment<Test>,
);

pub type UncheckedExtrinsic = TestXt<RuntimeCall, TransactionExtensions>;

impl frame_support::traits::OnRuntimeUpgrade for Test {
    fn on_runtime_upgrade() -> frame_support::weights::Weight {
        frame_support::weights::Weight::zero()
    }
}
impl frame_support::traits::BeforeAllRuntimeMigrations for Test {
    fn before_all_runtime_migrations() -> frame_support::weights::Weight {
        frame_support::weights::Weight::zero()
    }
}
impl frame_support::traits::OnInitialize<BlockNumber> for Test {
    fn on_initialize(_n: BlockNumber) -> frame_support::weights::Weight {
        frame_support::weights::Weight::zero()
    }
}
impl frame_support::traits::OnFinalize<BlockNumber> for Test {
    fn on_finalize(_n: BlockNumber) {}
}
impl frame_support::traits::OnIdle<BlockNumber> for Test {
    fn on_idle(
        _n: BlockNumber,
        _remaining_weight: frame_support::weights::Weight,
    ) -> frame_support::weights::Weight {
        frame_support::weights::Weight::zero()
    }
}
impl frame_support::traits::OnPoll<BlockNumber> for Test {
    fn on_poll(_n: BlockNumber, _remaining_weight: &mut frame_support::weights::WeightMeter) {}
}
impl frame_support::traits::OffchainWorker<BlockNumber> for Test {
    fn offchain_worker(_n: BlockNumber) {}
}

parameter_types! {
    pub const OperationalFeeMultiplier: u8 = 5;
    pub FeeMultiplier: Multiplier = Multiplier::one();
}

impl pallet_transaction_payment::Config for Test {
    type RuntimeEvent = RuntimeEvent;
    type OnChargeTransaction = SubtensorTxFeeHandler<Balances, TransactionFeeHandler<Test>>;
    // Convert dispatch weight to a chargeable fee.
    type WeightToFee = crate::LinearWeightToFee;
    type OperationalFeeMultiplier = OperationalFeeMultiplier;
    type LengthToFee = IdentityFee<Balance>;
    type FeeMultiplierUpdate = ConstFeeMultiplier<FeeMultiplier>;
    type WeightInfo = pallet_transaction_payment::weights::SubstrateWeight<Test>;
}

pub struct MockAuthorshipProvider;

pub const MOCK_BLOCK_BUILDER: u64 = 12345u64;

impl AuthorshipInfo<U256> for MockAuthorshipProvider {
    fn author() -> Option<U256> {
        Some(U256::from(MOCK_BLOCK_BUILDER))
    }
}

impl AuthorshipInfo<U256> for Test {
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
    pub const InitialTempo: u16 = 0;
    pub const SelfOwnership: u64 = 2;
    pub const InitialImmunityPeriod: u16 = 2;
    pub const InitialMinAllowedUids: u16 = 2;
    pub const InitialMaxAllowedUids: u16 = 4;
    pub const InitialBondsMovingAverage: u64 = 900_000;
    pub const InitialBondsPenalty: u16 = u16::MAX;
    pub const InitialBondsResetOn: bool = false;
    pub const InitialStakePruningMin: u16 = 0;
    pub const InitialFoundationDistribution: u64 = 0;
    pub const InitialDefaultDelegateTake: u16 = 11_796; // 18% honest number.
    pub const InitialMinDelegateTake: u16 = 5_898; // 9%;
    pub const InitialDefaultChildKeyTake: u16 = 0; // Allow 0 %
    pub const InitialMinChildKeyTake: u16 = 0; // Allow 0 %
    pub const InitialMaxChildKeyTake: u16 = 11_796; // 18 %;
    pub const InitialWeightsVersionKey: u16 = 0;
    pub const InitialServingRateLimit: u64 = 0; // No limit.
    pub const InitialTxRateLimit: u64 = 0; // Disable rate limit for testing
    pub const InitialTxDelegateTakeRateLimit: u64 = 0; // Disable rate limit for testing
    pub const InitialTxChildKeyTakeRateLimit: u64 = 0; // Disable rate limit for testing
    pub const InitialBurn: TaoBalance = TaoBalance::new(0);
    pub const InitialMinBurn: TaoBalance = TaoBalance::new(500_000);
    pub const InitialMinStake: TaoBalance = TaoBalance::new(2_000_000);
    pub const InitialMaxBurn: TaoBalance = TaoBalance::new(1_000_000_000);
    pub const MinBurnUpperBound: TaoBalance = TaoBalance::new(1_000_000_000); // 1 TAO
    pub const MaxBurnLowerBound: TaoBalance = TaoBalance::new(100_000_000); // 0.1 TAO
    pub const MinTempo: u16 = pallet_subtensor::MIN_TEMPO;
    pub const MaxTempo: u16 = pallet_subtensor::MAX_TEMPO;
    pub const MinActivityCutoffFactorMilli: u32 = pallet_subtensor::MIN_ACTIVITY_CUTOFF_FACTOR_MILLI;
    pub const MaxActivityCutoffFactorMilli: u32 = pallet_subtensor::MAX_ACTIVITY_CUTOFF_FACTOR_MILLI;
    pub const InitialValidatorPruneLen: u64 = 0;
    pub const InitialScalingLawPower: u16 = 50;
    pub const InitialMaxAllowedValidators: u16 = 100;
    pub const InitialIssuance: TaoBalance = TaoBalance::new(0);
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
    pub const InitialRAORecycledForRegistration: TaoBalance = TaoBalance::new(0);
    pub const InitialNetworkImmunityPeriod: u64 = 7200 * 7;
    pub const InitialNetworkMinLockCost: TaoBalance = TaoBalance::new(100_000_000_000_u64);
    pub const InitialSubnetOwnerCut: u16 = 0; // 0%. 100% of rewards go to validators + miners.
    pub const InitialNetworkLockReductionInterval: u64 = 2; // 2 blocks.
    pub const InitialNetworkRateLimit: u64 = 0;
    pub const InitialKeySwapCost: TaoBalance = TaoBalance::new(1_000_000_000);
    pub const InitialAlphaHigh: u16 = 58982; // Represents 0.9 as per the production default
    pub const InitialAlphaLow: u16 = 45875; // Represents 0.7 as per the production default
    pub const InitialLiquidAlphaOn: bool = false; // Default value for LiquidAlphaOn
    pub const InitialYuma3On: bool = false; // Default value for Yuma3On
    pub const InitialColdkeySwapAnnouncementDelay: u64 = 50;
    pub const InitialColdkeySwapReannouncementDelay: u64 = 10;
    pub const InitialDissolveNetworkScheduleDuration: u64 = 5 * 24 * 60 * 60 / 12; // 5 days
    pub const InitialTaoWeight: u64 = u64::MAX/10; // 10% global weight.
    pub const InitialEmaPriceHalvingPeriod: u64 = 201_600_u64; // 4 weeks
    pub const InitialStartCallDelay: u64 = 0; // 0 days
    pub const InitialKeySwapOnSubnetCost: TaoBalance = TaoBalance::new(10_000_000);
    pub const HotkeySwapOnSubnetInterval: u64 = 7 * 24 * 60 * 60 / 12; // 7 days
    pub const LeaseDividendsDistributionInterval: u32 = 100; // 100 blocks
    pub const MaxImmuneUidsPercentage: Percent = Percent::from_percent(80);
    pub const EvmKeyAssociateRateLimit: u64 = 0;
    pub const SubtensorPalletId: PalletId = PalletId(*b"subtensr");
    pub const BurnAccountId: PalletId = PalletId(*b"burntnsr");
    pub const MaxEpochsPerBlock: u8 = 32;
}

impl pallet_subtensor::Config for Test {
    type RuntimeCall = RuntimeCall;
    type Currency = Balances;
    type InitialIssuance = InitialIssuance;
    type SudoRuntimeCall = RuntimeCall;
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
    type InitialWeightsVersionKey = InitialWeightsVersionKey;
    type InitialMaxDifficulty = InitialMaxDifficulty;
    type InitialMinDifficulty = InitialMinDifficulty;
    type InitialServingRateLimit = InitialServingRateLimit;
    type InitialTxRateLimit = InitialTxRateLimit;
    type InitialTxDelegateTakeRateLimit = InitialTxDelegateTakeRateLimit;
    type InitialTxChildKeyTakeRateLimit = InitialTxChildKeyTakeRateLimit;
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
    type Preimages = ();
    type AlphaAssets = AlphaAssets;
    type InitialColdkeySwapAnnouncementDelay = InitialColdkeySwapAnnouncementDelay;
    type InitialColdkeySwapReannouncementDelay = InitialColdkeySwapReannouncementDelay;
    type InitialDissolveNetworkScheduleDuration = InitialDissolveNetworkScheduleDuration;
    type InitialTaoWeight = InitialTaoWeight;
    type InitialEmaPriceHalvingPeriod = InitialEmaPriceHalvingPeriod;
    type InitialStartCallDelay = InitialStartCallDelay;
    type SwapInterface = Swap;
    type KeySwapOnSubnetCost = InitialKeySwapOnSubnetCost;
    type HotkeySwapOnSubnetInterval = HotkeySwapOnSubnetInterval;
    type ProxyInterface = ();
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

parameter_types! {
    pub const CrowdloanPalletId: PalletId = PalletId(*b"bt/cloan");
    pub const MinimumDeposit: TaoBalance = TaoBalance::new(50);
    pub const AbsoluteMinimumContribution: TaoBalance = TaoBalance::new(10);
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

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl system::Config for Test {
    type BaseCallFilter = Everything;
    type BlockWeights = ();
    type BlockLength = ();
    type DbWeight = ();
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
    type Block = Block;
    type Nonce = u64;
}

impl pallet_grandpa::Config for Test {
    type RuntimeEvent = RuntimeEvent;

    type KeyOwnerProof = sp_core::Void;

    type WeightInfo = ();
    type MaxAuthorities = ConstU32<32>;
    type MaxSetIdSessionEntries = ConstU64<0>;
    type MaxNominators = ConstU32<20>;

    type EquivocationReportSystem = ();
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
    type RuntimeHoldReason = ();
}

impl pallet_alpha_assets::Config for Test {}

// Swap-related parameter types
parameter_types! {
    pub const SwapProtocolId: PalletId = PalletId(*b"ten/swap");
    pub const SwapMaxFeeRate: u16 = 10000; // 15.26%
    pub const SwapMinimumLiquidity: u64 = 1_000;
    pub const SwapMinimumReserve: NonZeroU64 = NonZeroU64::new(1_000_000).unwrap();
}

impl pallet_subtensor_swap::Config for Test {
    type SubnetInfo = SubtensorModule;
    type BalanceOps = SubtensorModule;
    type ProtocolId = SwapProtocolId;
    type TaoReserve = pallet_subtensor::TaoBalanceReserve<Self>;
    type AlphaReserve = pallet_subtensor::AlphaBalanceReserve<Self>;
    type MaxFeeRate = SwapMaxFeeRate;
    type MinimumLiquidity = SwapMinimumLiquidity;
    type MinimumReserve = SwapMinimumReserve;
    type WeightInfo = ();
    #[cfg(feature = "runtime-benchmarks")]
    type BenchmarkHelper = ();
}

pub struct OriginPrivilegeCmp;

impl PrivilegeCmp<OriginCaller> for OriginPrivilegeCmp {
    fn cmp_privilege(_left: &OriginCaller, _right: &OriginCaller) -> Option<Ordering> {
        None
    }
}

pub struct CommitmentsI;
impl pallet_subtensor::CommitmentsInterface for CommitmentsI {
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
    type Preimages = ();
    type BlockNumberProvider = System;
}

impl pallet_evm_chain_id::Config for Test {}
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

pub const KEY_TYPE: KeyTypeId = KeyTypeId(*b"test");

mod test_crypto {
    use super::KEY_TYPE;
    use sp_core::U256;
    use sp_core::sr25519::{Public as Sr25519Public, Signature as Sr25519Signature};
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

// Build genesis storage according to the mock runtime.
pub fn new_test_ext() -> sp_io::TestExternalities {
    sp_tracing::try_init_simple();
    let t = frame_system::GenesisConfig::<Test>::default()
        .build_storage()
        .unwrap();
    let mut ext = sp_io::TestExternalities::new(t);
    ext.execute_with(|| System::set_block_number(1));
    ext
}

#[allow(dead_code)]
pub(crate) fn run_to_block(n: u64) {
    while System::block_number() < n {
        SubtensorModule::on_finalize(System::block_number());
        System::on_finalize(System::block_number());
        System::reset_events();
        System::set_block_number(System::block_number() + 1);
        System::on_initialize(System::block_number());
        SubtensorModule::on_initialize(System::block_number());
    }
}

#[allow(dead_code)]
pub fn register_ok_neuron(
    netuid: NetUid,
    hotkey_account_id: U256,
    coldkey_account_id: U256,
    _start_nonce: u64,
) {
    // Ensure reserves exist for swap/burn path, but do NOT clobber reserves if the test already set them.
    let reserve: u64 = 1_000_000_000_000;
    let tao_reserve = SubnetTAO::<Test>::get(netuid);
    let alpha_reserve = SubnetAlphaIn::<Test>::get(netuid);

    if tao_reserve.is_zero() && alpha_reserve.is_zero() {
        setup_reserves(netuid, reserve.into(), reserve.into());
    }

    // Ensure coldkey has enough to pay the current burn AND is not fully drained to zero.
    // This avoids ZeroBalanceAfterWithdrawn in burned_register.
    let top_up_for_burn = |netuid: NetUid, cold: U256| {
        let burn: TaoBalance = SubtensorModule::get_burn(netuid);

        // Make sure something remains after withdrawal even if ED is 0 in tests.
        let ed: TaoBalance = ExistentialDeposit::get();
        let min_remaining: TaoBalance = ed.max(1.into());

        // Small buffer for safety (fees / rounding / future changes).
        let buffer: TaoBalance = 10.into();

        let min_balance_needed: TaoBalance = burn + min_remaining + buffer;

        let bal: TaoBalance = SubtensorModule::get_coldkey_balance(&cold);
        if bal < min_balance_needed {
            add_balance_to_coldkey_account(&cold, min_balance_needed - bal);
        }
    };

    top_up_for_burn(netuid, coldkey_account_id);

    let origin = <<Test as frame_system::Config>::RuntimeOrigin>::signed(coldkey_account_id);
    let result = SubtensorModule::burned_register(origin.clone(), netuid, hotkey_account_id);

    match result {
        Ok(()) => {
            // success
        }
        Err(e)
            if e == Error::<Test>::TooManyRegistrationsThisInterval.into()
                || e == Error::<Test>::NotEnoughBalanceToStake.into()
                || e == Error::<Test>::ZeroBalanceAfterWithdrawn.into() =>
        {
            // Re-top-up and retry once (burn can be state-dependent).
            top_up_for_burn(netuid, coldkey_account_id);

            assert_ok!(SubtensorModule::burned_register(
                origin,
                netuid,
                hotkey_account_id
            ));
        }
        Err(e) => {
            panic!("Expected Ok(_). Got Err({e:?})");
        }
    }

    log::info!(
        "Register ok neuron: netuid: {netuid:?}, coldkey: {coldkey_account_id:?}, hotkey: {hotkey_account_id:?}"
    );
}

#[allow(dead_code)]
pub fn add_balance_to_coldkey_account(coldkey: &U256, tao: TaoBalance) {
    let credit = SubtensorModule::mint_tao(tao);
    let _ = SubtensorModule::spend_tao(coldkey, credit, tao).unwrap();
}

#[allow(dead_code)]
pub fn remove_balance_from_coldkey_account(coldkey: &U256, tao: TaoBalance) {
    let _ = SubtensorModule::burn_tao(coldkey, tao);
}

#[allow(dead_code)]
pub fn add_dynamic_network(hotkey: &U256, coldkey: &U256) -> NetUid {
    let netuid = SubtensorModule::get_next_netuid();
    let lock_cost = SubtensorModule::get_network_lock_cost();
    add_balance_to_coldkey_account(coldkey, lock_cost.into());

    assert_ok!(SubtensorModule::register_network(
        RawOrigin::Signed(*coldkey).into(),
        *hotkey
    ));
    NetworkRegistrationAllowed::<Test>::insert(netuid, true);
    NetworkPowRegistrationAllowed::<Test>::insert(netuid, true);
    FirstEmissionBlockNumber::<Test>::insert(netuid, 0);
    SubtokenEnabled::<Test>::insert(netuid, true);
    netuid
}

pub(crate) fn setup_reserves(netuid: NetUid, tao: TaoBalance, alpha: AlphaBalance) {
    SubnetTAO::<Test>::set(netuid, tao);
    SubnetAlphaIn::<Test>::set(netuid, alpha);
}

pub(crate) fn swap_alpha_to_tao_ext(
    netuid: NetUid,
    alpha: AlphaBalance,
    drop_fees: bool,
) -> (u64, u64) {
    if netuid.is_root() {
        return (alpha.into(), 0);
    }

    let order = GetTaoForAlpha::<Test>::with_amount(alpha);
    let result = <Test as pallet::Config>::SwapInterface::swap(
        netuid.into(),
        order,
        <Test as pallet::Config>::SwapInterface::min_price(),
        drop_fees,
        true,
    );

    assert_ok!(&result);

    let result = result.unwrap();

    // we don't want to have silent 0 comparisons in tests
    assert!(!result.amount_paid_out.is_zero());

    (result.amount_paid_out.to_u64(), result.fee_paid.to_u64())
}

pub(crate) fn swap_alpha_to_tao(netuid: NetUid, alpha: AlphaBalance) -> (u64, u64) {
    swap_alpha_to_tao_ext(netuid, alpha, false)
}

pub(crate) fn swap_tao_to_alpha_ext(
    netuid: NetUid,
    tao: TaoBalance,
    drop_fees: bool,
) -> (u64, u64) {
    if netuid.is_root() {
        return (tao.into(), 0);
    }

    let order = GetAlphaForTao::<Test>::with_amount(tao);
    let result = <Test as pallet::Config>::SwapInterface::swap(
        netuid.into(),
        order,
        <Test as pallet::Config>::SwapInterface::max_price(),
        drop_fees,
        true,
    );

    assert_ok!(&result);

    let result = result.unwrap();

    // we don't want to have silent 0 comparisons in tests
    assert!(!result.amount_paid_out.is_zero());

    (result.amount_paid_out.to_u64(), result.fee_paid.to_u64())
}

pub(crate) fn swap_tao_to_alpha(netuid: NetUid, tao: TaoBalance) -> (u64, u64) {
    swap_tao_to_alpha_ext(netuid, tao, false)
}

#[allow(dead_code)]
pub fn add_network(netuid: NetUid, tempo: u16) {
    SubtensorModule::init_new_network(netuid, tempo);
    SubtensorModule::set_network_registration_allowed(netuid, true);
}

#[allow(dead_code)]
pub struct TestSubnet {
    pub netuid: NetUid,
    pub ck_owner: U256,
    pub hk_owner: U256,
}

#[allow(dead_code)]
pub struct TestSetup {
    pub subnets: Vec<TestSubnet>,
    pub coldkey: U256,
    pub hotkeys: Vec<U256>,
}

#[allow(dead_code)]
pub fn setup_subnets(sncount: u16, neurons: u16) -> TestSetup {
    let mut subnets: Vec<TestSubnet> = Vec::new();
    let owner_ck_start_id = 100;
    let owner_hk_start_id = 200;
    let coldkey = U256::from(10000);
    let neuron_hk_start_id = 20000;
    let amount = 1_000_000_000_000_u64;
    let mut hotkeys: Vec<U256> = Vec::new();

    for sn in 0..sncount {
        let cko = U256::from(owner_ck_start_id + sn);
        let hko = U256::from(owner_hk_start_id + sn);

        // Create subnet
        let subnet = TestSubnet {
            netuid: add_dynamic_network(&cko, &hko),
            ck_owner: cko,
            hk_owner: hko,
        };

        // Set tempo to 10 blocks
        Tempo::<Test>::insert(subnet.netuid, 10);

        // Add neurons (all the same for all subnets)
        for uid in 1..=neurons {
            let hotkey = U256::from(neuron_hk_start_id + uid);
            register_ok_neuron(subnet.netuid, hotkey, coldkey, 192213123);
            hotkeys.push(hotkey);
        }

        // Setup pool reserves
        setup_reserves(subnet.netuid, amount.into(), amount.into());

        // Cause the v3 pool to initialize
        SubtensorModule::swap_tao_for_alpha(
            subnet.netuid,
            0.into(),
            1_000_000_000_000_u64.into(),
            false,
        )
        .unwrap();

        subnets.push(subnet);
    }

    TestSetup {
        subnets,
        coldkey,
        hotkeys,
    }
}

#[allow(dead_code)]
pub fn setup_stake(
    netuid: subtensor_runtime_common::NetUid,
    coldkey: &U256,
    hotkey: &U256,
    stake_amount: u64,
) {
    // Fund enough to stake while keeping the coldkey account alive (KeepAlive / ED).
    add_balance_to_coldkey_account(
        coldkey,
        TaoBalance::from(stake_amount) + ExistentialDeposit::get(),
    );

    assert_ok!(SubtensorModule::add_stake(
        RuntimeOrigin::signed(*coldkey),
        *hotkey,
        netuid,
        stake_amount.into(),
    ));
}

pub(crate) fn quote_remove_stake_after_alpha_fee(
    coldkey: &U256,
    hotkey: &U256,
    netuid: NetUid,
    alpha: AlphaBalance,
) -> (u64, u64) {
    if netuid.is_root() {
        return (alpha.into(), 0);
    }

    let call: RuntimeCall = RuntimeCall::SubtensorModule(pallet_subtensor::Call::remove_stake {
        hotkey: *hotkey,
        netuid,
        amount_unstaked: alpha,
    });
    let info = <RuntimeCall as frame_support::dispatch::GetDispatchInfo>::get_dispatch_info(&call);
    let tao_fee = pallet_transaction_payment::Pallet::<Test>::compute_fee(0, &info, 0.into());

    frame_support::storage::with_transaction(
        || -> frame_support::storage::TransactionOutcome<
            Result<(u64, u64), sp_runtime::DispatchError>,
        > {
            let alpha_balance =
                SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(hotkey, coldkey, netuid);

            let mut alpha_fee =
                pallet_subtensor_swap::Pallet::<Test>::get_alpha_amount_for_tao(
                    netuid,
                    tao_fee.into(),
                );

            if alpha_fee.is_zero() {
                alpha_fee = alpha_balance;
            }

            let alpha_fee = alpha_fee.min(alpha_balance);

            if !alpha_fee.is_zero() {
                assert_ok!(SubtensorModule::unstake_from_subnet(
                    hotkey,
                    coldkey,
                    coldkey,
                    netuid,
                    alpha_fee,
                    0.into(),
                    true,
                ));
            }

            let alpha_after_fee =
                SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                    hotkey, coldkey, netuid,
                );
            let quoted_alpha = alpha.min(alpha_after_fee);

            let quote = swap_alpha_to_tao(netuid, quoted_alpha);
            frame_support::storage::TransactionOutcome::Rollback(Ok(quote))
        },
    )
    .expect("transactional quote should not fail")
}
