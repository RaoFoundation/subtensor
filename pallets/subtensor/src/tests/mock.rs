#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::unwrap_used
)]

use core::num::NonZeroU64;

use crate::root_registered::{
    EmaValueProvider, OnRootRegistrationChange, RootRegisteredInspector, SampleStep,
};
use crate::utils::rate_limiting::TransactionType;
use crate::*;
pub use frame_support::traits::Imbalance;
use frame_support::traits::{Contains, Everything, InsideBoth, InstanceFilter};
use frame_support::weights::Weight;
use frame_support::weights::constants::RocksDbWeight;
use frame_support::{PalletId, derive_impl};
use frame_support::{
    assert_ok, parameter_types,
    traits::{Hooks, PrivilegeCmp},
};
use frame_system as system;
use frame_system::{EnsureRoot, RawOrigin, limits};
use pallet_subtensor_proxy as pallet_proxy;
use pallet_subtensor_utility as pallet_utility;
use share_pool::SafeFloat;
use sp_core::{ConstU64, Get, H256, U256, offchain::KeyTypeId};
use sp_runtime::Perbill;
use sp_runtime::{
    BuildStorage, Percent,
    traits::{BlakeTwo256, IdentityLookup},
};
use sp_std::{cell::RefCell, cmp::Ordering, sync::OnceLock};
use sp_tracing::tracing_subscriber;
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::{AuthorshipInfo, NetUid, TaoBalance};
use subtensor_swap_interface::{Order, SwapHandler};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};
type Block = frame_system::mocking::MockBlock<Test>;

// Configure a mock runtime to test the pallet.
frame_support::construct_runtime!(
    pub enum Test
    {
        System: frame_system = 1,
        Balances: pallet_balances = 2,
        Shield: pallet_shield = 3,
        SubtensorModule: crate = 4,
        AlphaAssets: pallet_alpha_assets = 5,
        Utility: pallet_utility = 6,
        Scheduler: pallet_scheduler = 7,
        Preimage: pallet_preimage = 8,
        Drand: pallet_drand = 9,
        Swap: pallet_subtensor_swap = 10,
        Crowdloan: pallet_crowdloan = 11,
        Proxy: pallet_subtensor_proxy = 12,
    }
);

#[allow(dead_code)]
pub type SubtensorCall = crate::Call<Test>;

#[allow(dead_code)]
pub type SubtensorEvent = crate::Event<Test>;

#[allow(dead_code)]
pub type BalanceCall = pallet_balances::Call<Test>;

#[allow(dead_code)]
pub type TestRuntimeCall = frame_system::Call<Test>;

pub const KEY_TYPE: KeyTypeId = KeyTypeId(*b"test");

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

impl pallet_alpha_assets::Config for Test {}

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
        crate::CheckColdkeySwap<Test>,
        crate::CheckWeights<Test>,
        crate::CheckRateLimits<Test>,
        crate::CheckDelegateTake<Test>,
        crate::CheckServingEndpoints<Test>,
        crate::CheckEvmKeyAssociation<Test>,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RootRegistrationChange {
    Added(U256),
    Removed(U256),
}

thread_local! {
    static ROOT_REGISTRATION_LOG: core::cell::RefCell<Vec<RootRegistrationChange>> =
        const { core::cell::RefCell::new(Vec::new()) };
}

pub fn take_root_registration_log() -> Vec<RootRegistrationChange> {
    ROOT_REGISTRATION_LOG.with(|log| log.borrow_mut().drain(..).collect())
}

pub struct MockOnRootRegistrationChange;

impl OnRootRegistrationChange<U256> for MockOnRootRegistrationChange {
    fn on_added(coldkey: &U256) {
        ROOT_REGISTRATION_LOG.with(|log| {
            log.borrow_mut()
                .push(RootRegistrationChange::Added(*coldkey))
        });
    }
    fn on_removed(coldkey: &U256) {
        ROOT_REGISTRATION_LOG.with(|log| {
            log.borrow_mut()
                .push(RootRegistrationChange::Removed(*coldkey))
        });
    }
    fn on_added_weight() -> Weight {
        Weight::zero()
    }
    fn on_removed_weight() -> Weight {
        Weight::zero()
    }
}

thread_local! {
    static MOCK_ROOT_REGISTERED_INSPECTOR_MEMBERS: core::cell::RefCell<Option<Vec<U256>>> =
        const { core::cell::RefCell::new(None) };
}

/// Override the membership exposed by `MockRootRegisteredInspector` to
/// `pallet_subtensor`'s try_state check. `None` (the default) makes
/// the check a no-op; `Some(_)` opts the test in.
pub fn set_mock_root_registered_inspector_members(members: Option<Vec<U256>>) {
    MOCK_ROOT_REGISTERED_INSPECTOR_MEMBERS.with(|m| *m.borrow_mut() = members);
}

pub struct MockRootRegisteredInspector;

impl RootRegisteredInspector<U256> for MockRootRegisteredInspector {
    fn members() -> Option<Vec<U256>> {
        MOCK_ROOT_REGISTERED_INSPECTOR_MEMBERS.with(|m| m.borrow().clone())
    }
}

thread_local! {
    static EMA_VALUE_PROVIDER_LOG: RefCell<Vec<(U256, U64F64)>> =
        const { RefCell::new(Vec::new()) };
}

pub fn take_ema_value_provider_log() -> Vec<(U256, U64F64)> {
    EMA_VALUE_PROVIDER_LOG.with(|log| log.borrow_mut().drain(..).collect())
}

/// Define a thread-local whose value can be temporarily replaced via an
/// RAII guard. The previous value is restored when the guard drops, so
/// tests do not need to manually undo their setup (and inherit nothing
/// from a panicking neighbor).
macro_rules! define_scoped_state {
    ($flag:ident, $guard:ident, $reader:ident, $ty:ty, $default:expr) => {
        thread_local! {
            static $flag: RefCell<$ty> = const { RefCell::new($default) };
        }

        #[must_use = "the guard restores the prior value on drop; bind it to a local"]
        pub struct $guard {
            previous: Option<$ty>,
        }

        impl $guard {
            pub fn new(value: $ty) -> Self {
                let previous =
                    Some($flag.with(|r| core::mem::replace(&mut *r.borrow_mut(), value)));
                Self { previous }
            }
        }

        impl Drop for $guard {
            fn drop(&mut self) {
                if let Some(prev) = self.previous.take() {
                    $flag.with(|r| *r.borrow_mut() = prev);
                }
            }
        }

        fn $reader() -> $ty {
            $flag.with(|r| r.borrow().clone())
        }
    };
}

define_scoped_state!(
    EMA_VALUE_PROVIDER_STEP,
    EmaValueProviderStepGuard,
    ema_value_provider_step,
    Option<fn(U256, MockEmaProgress) -> (SampleStep<MockEmaProgress>, Weight)>,
    None
);
define_scoped_state!(
    EMA_VALUE_PROVIDER_STEP_WEIGHT,
    EmaValueProviderStepWeightGuard,
    ema_value_provider_step_weight,
    Weight,
    Weight::zero()
);

#[freeze_struct("79e67cd33ad5c63b")]
#[derive(
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    Debug,
    Encode,
    Decode,
    DecodeWithMemTracking,
    MaxEncodedLen,
    TypeInfo,
)]
pub struct MockEmaProgress {
    pub offset: u32,
    pub partial: u128,
}

pub struct MockEmaValueProvider;

impl EmaValueProvider<U256> for MockEmaValueProvider {
    type Progress = MockEmaProgress;

    fn step(coldkey: &U256, progress: Self::Progress) -> (SampleStep<Self::Progress>, Weight) {
        let (step, weight) = match ema_value_provider_step() {
            Some(f) => f(*coldkey, progress),
            None => (
                SampleStep::Complete {
                    sample: U64F64::from_num(0u64),
                },
                ema_value_provider_step_weight(),
            ),
        };
        EMA_VALUE_PROVIDER_LOG
            .with(|log| log.borrow_mut().push((*coldkey, U64F64::from_num(0u64))));
        (step, weight)
    }

    fn step_weight() -> Weight {
        ema_value_provider_step_weight()
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
    pub const MinTempo: u16 = crate::MIN_TEMPO;
    pub const MaxTempo: u16 = crate::MAX_TEMPO;
    pub const MinActivityCutoffFactorMilli: u32 = crate::MIN_ACTIVITY_CUTOFF_FACTOR_MILLI;
    pub const MaxActivityCutoffFactorMilli: u32 = crate::MAX_ACTIVITY_CUTOFF_FACTOR_MILLI;
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

impl crate::Config for Test {
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
    type AlphaAssets = AlphaAssets;
    type EvmKeyAssociateRateLimit = EvmKeyAssociateRateLimit;
    type AuthorshipProvider = MockAuthorshipProvider;
    type OnRootRegistrationChange = MockOnRootRegistrationChange;
    type RootRegisteredInspector = MockRootRegisteredInspector;
    type EmaValueProvider = MockEmaValueProvider;
    type SubtensorPalletId = SubtensorPalletId;
    type BurnAccountId = BurnAccountId;
    type InitialMaxEpochsPerBlock = MaxEpochsPerBlock;
    type WeightInfo = ();
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
    #[cfg(feature = "runtime-benchmarks")]
    type BenchmarkHelper = ();
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

pub const RAO_PER_TAO: u64 = 1_000_000_000;
pub const DEFAULT_RESERVE: u64 = 1_000_000_000_000;

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
// Build genesis storage according to the mock runtime.
pub fn new_test_ext(block_number: BlockNumber) -> sp_io::TestExternalities {
    init_logs_for_tests();
    let t = frame_system::GenesisConfig::<Test>::default()
        .build_storage()
        .unwrap();
    let mut ext = sp_io::TestExternalities::new(t);
    ext.execute_with(|| System::set_block_number(block_number));
    ext
}

#[allow(dead_code)]
pub fn test_ext_with_balances(balances: Vec<(U256, u128)>) -> sp_io::TestExternalities {
    init_logs_for_tests();
    let mut t = frame_system::GenesisConfig::<Test>::default()
        .build_storage()
        .unwrap();

    pallet_balances::GenesisConfig::<Test> {
        balances: balances
            .iter()
            .map(|(a, b)| (*a, TaoBalance::from(*b as u64)))
            .collect::<Vec<(U256, TaoBalance)>>(),
        dev_accounts: None,
    }
    .assimilate_storage(&mut t)
    .unwrap();

    t.into()
}

#[allow(dead_code)]
pub(crate) fn step_block(n: u16) {
    for _ in 0..n {
        Scheduler::on_finalize(System::block_number());
        Proxy::on_finalize(System::block_number());
        SubtensorModule::on_finalize(System::block_number());
        System::on_finalize(System::block_number());
        System::set_block_number(System::block_number() + 1);
        System::on_initialize(System::block_number());
        SubtensorModule::on_initialize(System::block_number());
        Scheduler::on_initialize(System::block_number());
    }
}

#[allow(dead_code)]
pub(crate) fn run_to_block(n: u64) {
    run_to_block_ext(n, false)
}

#[allow(dead_code)]
pub(crate) fn run_to_block_ext(n: u64, enable_events: bool) {
    while System::block_number() < n {
        Scheduler::on_finalize(System::block_number());
        SubtensorModule::on_finalize(System::block_number());
        System::on_finalize(System::block_number());
        System::set_block_number(System::block_number() + 1);
        System::on_initialize(System::block_number());
        if !enable_events {
            System::events().iter().for_each(|event| {
                log::info!("Event: {:?}", event.event);
            });
            System::reset_events();
        }
        SubtensorModule::on_initialize(System::block_number());
        Scheduler::on_initialize(System::block_number());
    }
}

#[allow(dead_code)]
pub(crate) fn next_block_no_epoch(netuid: NetUid) -> u64 {
    // high tempo to skip automatic epochs in on_initialize
    let high_tempo: u16 = u16::MAX - 1;
    let old_tempo: u16 = SubtensorModule::get_tempo(netuid);

    SubtensorModule::set_tempo_unchecked(netuid, high_tempo);
    let new_block = next_block();
    SubtensorModule::set_tempo_unchecked(netuid, old_tempo);

    new_block
}

#[allow(dead_code)]
pub(crate) fn run_to_block_no_epoch(netuid: NetUid, n: u64) {
    // high tempo to skip automatic epochs in on_initialize
    let high_tempo: u16 = u16::MAX - 1;
    let old_tempo: u16 = SubtensorModule::get_tempo(netuid);

    SubtensorModule::set_tempo_unchecked(netuid, high_tempo);
    run_to_block(n);
    SubtensorModule::set_tempo_unchecked(netuid, old_tempo);
}

#[allow(dead_code)]
pub(crate) fn step_epochs(count: u16, netuid: NetUid) {
    const STEP_EPOCHS_MAX_BLOCKS: u32 = 50_000;

    // Advance block-by-block until exactly `count` more epoch slots have been
    // consumed for `netuid`, observed via the `SubnetEpochIndex` counter. Robust
    // to any tempo (including `tempo == 1`) and to the per-block epoch cap.
    let target = crate::SubnetEpochIndex::<Test>::get(netuid) + count as u64;
    let mut blocks_advanced: u32 = 0;
    while crate::SubnetEpochIndex::<Test>::get(netuid) < target {
        step_block(1);
        blocks_advanced += 1;
        assert!(
            blocks_advanced < STEP_EPOCHS_MAX_BLOCKS,
            "step_epochs: epoch counter never advanced (tempo == 0?)"
        );
    }
}

/// Increments current block by 1, running all hooks associated with doing so, and asserts
/// that the block number was in fact incremented.
///
/// Returns the new block number.
#[allow(dead_code)]
#[cfg(test)]
pub(crate) fn next_block() -> u64 {
    let mut block = System::block_number();
    block += 1;
    run_to_block(block);
    assert_eq!(System::block_number(), block);
    block
}

pub fn register_ok_neuron(
    netuid: NetUid,
    hotkey_account_id: U256,
    coldkey_account_id: U256,
    _start_nonce: u64,
) {
    SubtensorModule::set_burn(netuid, TaoBalance::from(0));
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
        let burn_u64: TaoBalance = burn;

        // Make sure something remains after withdrawal even if ED is 0 in tests.
        let ed: TaoBalance = ExistentialDeposit::get();
        let min_remaining: TaoBalance = ed.max(1.into());

        // Small buffer for safety (fees / rounding / future changes).
        let buffer: TaoBalance = 10.into();

        let min_balance_needed: TaoBalance = burn_u64 + min_remaining + buffer;

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
    SubtensorModule::set_burn(netuid, TaoBalance::from(0));
    log::info!(
        "Register ok neuron: netuid: {netuid:?}, coldkey: {coldkey_account_id:?}, hotkey: {hotkey_account_id:?}"
    );
}

#[allow(dead_code)]
pub fn add_network(netuid: NetUid, tempo: u16, _modality: u16) {
    SubtensorModule::init_new_network(netuid, tempo);
    SubtensorModule::set_network_registration_allowed(netuid, true);
    FirstEmissionBlockNumber::<Test>::insert(netuid, 1);
    SubtokenEnabled::<Test>::insert(netuid, true);

    // make interval 1 block so tests can register by stepping 1 block.
    BurnHalfLife::<Test>::insert(netuid, 1);
    BurnIncreaseMult::<Test>::insert(netuid, U64F64::from_num(1));
}

#[allow(dead_code)]
pub fn add_network_without_emission_block(netuid: NetUid, tempo: u16, _modality: u16) {
    SubtensorModule::init_new_network(netuid, tempo);
    SubtensorModule::set_network_registration_allowed(netuid, true);

    BurnHalfLife::<Test>::insert(netuid, 1);
    BurnIncreaseMult::<Test>::insert(netuid, U64F64::from_num(1));
}

#[allow(dead_code)]
pub fn add_network_disable_subtoken(netuid: NetUid, tempo: u16, _modality: u16) {
    SubtensorModule::init_new_network(netuid, tempo);
    SubtensorModule::set_network_registration_allowed(netuid, true);

    SubtokenEnabled::<Test>::insert(netuid, false);

    BurnHalfLife::<Test>::insert(netuid, 1);
    BurnIncreaseMult::<Test>::insert(netuid, U64F64::from_num(1));
}

#[allow(dead_code)]
pub fn add_dynamic_network(hotkey: &U256, coldkey: &U256) -> NetUid {
    let netuid = SubtensorModule::get_next_netuid();
    let lock_cost = SubtensorModule::get_network_lock_cost();
    add_balance_to_coldkey_account(coldkey, lock_cost.into());
    TotalIssuance::<Test>::mutate(|total_issuance| {
        *total_issuance = total_issuance.saturating_add(lock_cost);
    });

    assert_ok!(SubtensorModule::register_network(
        RawOrigin::Signed(*coldkey).into(),
        *hotkey
    ));
    NetworkRegistrationAllowed::<Test>::insert(netuid, true);
    FirstEmissionBlockNumber::<Test>::insert(netuid, 0);
    SubtokenEnabled::<Test>::insert(netuid, true);

    // make interval 1 block so tests can register by stepping 1 block.
    BurnHalfLife::<Test>::insert(netuid, 1);
    BurnIncreaseMult::<Test>::insert(netuid, U64F64::from_num(1));

    netuid
}

#[allow(dead_code)]
pub fn add_dynamic_network_without_emission_block(hotkey: &U256, coldkey: &U256) -> NetUid {
    let netuid = SubtensorModule::get_next_netuid();
    let lock_cost = SubtensorModule::get_network_lock_cost();
    add_balance_to_coldkey_account(coldkey, lock_cost.into());
    TotalIssuance::<Test>::mutate(|total_issuance| {
        *total_issuance = total_issuance.saturating_add(lock_cost);
    });

    assert_ok!(SubtensorModule::register_network(
        RawOrigin::Signed(*coldkey).into(),
        *hotkey
    ));

    NetworkRegistrationAllowed::<Test>::insert(netuid, true);
    BurnHalfLife::<Test>::insert(netuid, 1);
    BurnIncreaseMult::<Test>::insert(netuid, U64F64::from_num(1));

    netuid
}

#[allow(dead_code)]
pub fn add_dynamic_network_disable_commit_reveal(hotkey: &U256, coldkey: &U256) -> NetUid {
    let netuid = add_dynamic_network(hotkey, coldkey);
    SubtensorModule::set_commit_reveal_weights_enabled(netuid, false);
    netuid
}

#[allow(dead_code)]
pub fn add_network_disable_commit_reveal(netuid: NetUid, tempo: u16, _modality: u16) {
    add_network(netuid, tempo, _modality);
    SubtensorModule::set_commit_reveal_weights_enabled(netuid, false);
    SubtensorModule::set_yuma3_enabled(netuid, false);
}

// Helper function to set up a neuron with stake
#[allow(dead_code)]
pub fn setup_neuron_with_stake(netuid: NetUid, hotkey: U256, coldkey: U256, stake: TaoBalance) {
    register_ok_neuron(netuid, hotkey, coldkey, stake.into());
    increase_stake_on_coldkey_hotkey_account(&coldkey, &hotkey, stake, netuid);
}

#[allow(dead_code)]
pub fn wait_set_pending_children_cooldown(netuid: NetUid) {
    let cooldown = DefaultPendingCooldown::<Test>::get();
    step_block(cooldown as u16); // Wait for cooldown to pass
    step_epochs(1, netuid); // Run next epoch
}

#[allow(dead_code)]
pub fn wait_and_set_pending_children(netuid: NetUid) {
    let original_block = System::block_number();
    wait_set_pending_children_cooldown(netuid);
    SubtensorModule::do_set_pending_children(netuid);
    System::set_block_number(original_block);
}

#[allow(dead_code)]
pub fn mock_schedule_children(
    coldkey: &U256,
    parent: &U256,
    netuid: NetUid,
    child_vec: &[(u64, U256)],
) {
    // Set minimum stake for setting children
    StakeThreshold::<Test>::put(0);

    // Set initial parent-child relationship
    assert_ok!(SubtensorModule::do_schedule_children(
        RuntimeOrigin::signed(*coldkey),
        *parent,
        netuid,
        child_vec.to_vec()
    ));
}

#[allow(dead_code)]
pub fn mock_set_children(coldkey: &U256, parent: &U256, netuid: NetUid, child_vec: &[(u64, U256)]) {
    mock_schedule_children(coldkey, parent, netuid, child_vec);
    wait_and_set_pending_children(netuid);
}

#[allow(dead_code)]
pub fn mock_set_children_no_epochs(netuid: NetUid, parent: &U256, child_vec: &[(u64, U256)]) {
    let backup_block = SubtensorModule::get_current_block_as_u64();
    PendingChildKeys::<Test>::insert(netuid, parent, (child_vec, 0));
    FirstEmissionBlockNumber::<Test>::insert(netuid, 0);
    let cooldown = PendingChildKeyCooldown::<Test>::get();
    System::set_block_number(cooldown + 1);
    SubtensorModule::do_set_pending_children(netuid);
    System::set_block_number(backup_block);
}

// Helper function to wait for the rate limit
#[allow(dead_code)]
pub fn step_rate_limit(transaction_type: &TransactionType, netuid: NetUid) {
    // Check rate limit
    let limit = transaction_type.rate_limit_on_subnet::<Test>(netuid);

    // Step that many blocks
    step_block(limit as u16);
}

/// Helper function to mock now missing increase_stake_on_coldkey_hotkey_account with
/// minimal changes
#[allow(dead_code)]
pub fn increase_stake_on_coldkey_hotkey_account(
    coldkey: &U256,
    hotkey: &U256,
    tao_staked: TaoBalance,
    netuid: NetUid,
) {
    // Add TAO balance to coldkey account
    add_balance_to_coldkey_account(coldkey, tao_staked.into());

    // Stake
    SubtensorModule::stake_into_subnet(
        hotkey,
        coldkey,
        netuid,
        tao_staked,
        <Test as Config>::SwapInterface::max_price(),
        false,
    )
    .unwrap();
}

/// Increases the stake on the hotkey account under its owning coldkey.
///
/// # Arguments
/// * `hotkey` - The hotkey account ID.
/// * `increment` - The amount to be incremented.
#[allow(dead_code)]
pub fn increase_stake_on_hotkey_account(hotkey: &U256, increment: TaoBalance, netuid: NetUid) {
    increase_stake_on_coldkey_hotkey_account(
        &SubtensorModule::get_owning_coldkey_for_hotkey(hotkey),
        hotkey,
        increment,
        netuid,
    );
}

pub(crate) fn setup_reserves(netuid: NetUid, tao: TaoBalance, alpha: AlphaBalance) {
    SubnetTAO::<Test>::set(netuid, tao);
    SubnetAlphaIn::<Test>::set(netuid, alpha);
}

pub(crate) fn swap_tao_to_alpha(netuid: NetUid, tao: TaoBalance) -> (AlphaBalance, u64) {
    if netuid.is_root() {
        return (tao.to_u64().into(), 0);
    }

    let order = GetAlphaForTao::<Test>::with_amount(tao);
    let result = <Test as pallet::Config>::SwapInterface::swap(
        netuid.into(),
        order,
        <Test as pallet::Config>::SwapInterface::max_price(),
        false,
        true,
    );

    assert_ok!(&result);

    let result = result.unwrap();

    // we don't want to have silent 0 comparisons in tests
    assert!(result.amount_paid_out > AlphaBalance::ZERO);

    (result.amount_paid_out, result.fee_paid.into())
}

pub(crate) fn swap_alpha_to_tao_ext(
    netuid: NetUid,
    alpha: AlphaBalance,
    drop_fees: bool,
) -> (TaoBalance, u64) {
    if netuid.is_root() {
        return (alpha.to_u64().into(), 0);
    }

    println!(
        "<Test as pallet::Config>::SwapInterface::min_price() = {:?}",
        <Test as pallet::Config>::SwapInterface::min_price::<TaoBalance>()
    );

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

    (result.amount_paid_out, result.fee_paid.into())
}

pub(crate) fn swap_alpha_to_tao(netuid: NetUid, alpha: AlphaBalance) -> (TaoBalance, u64) {
    swap_alpha_to_tao_ext(netuid, alpha, false)
}

#[allow(dead_code)]
pub(crate) fn last_event() -> RuntimeEvent {
    System::events().pop().expect("RuntimeEvent expected").event
}

pub fn assert_last_event<T: frame_system::pallet::Config>(
    generic_event: <T as frame_system::pallet::Config>::RuntimeEvent,
) {
    frame_system::Pallet::<T>::assert_last_event(generic_event.into());
}

#[allow(dead_code)]
pub fn commit_dummy(who: U256, netuid: NetUid) {
    SubtensorModule::set_weights_set_rate_limit(netuid, 0);

    // any 32‑byte value is fine; hash is never opened
    let hash = sp_core::H256::from_low_u64_be(0xDEAD_BEEF);
    assert_ok!(SubtensorModule::do_commit_weights(
        RuntimeOrigin::signed(who),
        netuid,
        hash
    ));
}

#[allow(dead_code)]
pub fn sf_to_u128(sf: &SafeFloat) -> u128 {
    let alpha_f64: f64 = sf.into();
    alpha_f64 as u128
}

#[allow(dead_code)]
pub fn sf_from_u64(val: u64) -> SafeFloat {
    SafeFloat::from(val)
}

#[allow(dead_code)]
pub fn add_balance_to_coldkey_account(coldkey: &U256, tao: TaoBalance) {
    let ed = ExistentialDeposit::get();
    if tao >= ed {
        let credit = SubtensorModule::mint_tao(tao);
        let _ = SubtensorModule::spend_tao(coldkey, credit, tao).unwrap();
    }
}

#[allow(dead_code)]
pub fn remove_balance_from_coldkey_account(coldkey: &U256, tao: TaoBalance) {
    let _ = SubtensorModule::burn_tao(coldkey, tao);
}

#[allow(dead_code)]
pub fn mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
    hotkey: &U256,
    coldkey: &U256,
    netuid: NetUid,
    alpha: AlphaBalance,
) {
    // Record stake in alpha pool
    SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
        hotkey, coldkey, netuid, alpha,
    );

    // Make sure subnet exists, so does it's account
    NetworksAdded::<Test>::insert(netuid, true);

    // Add TAO balance to subnet account
    // For simplicity make it equal to alpha * 100, which is more than needed
    let subnet_account = SubtensorModule::get_subnet_account_id(netuid).unwrap();
    let tao_bal = u64::from(alpha) * 100;
    add_balance_to_coldkey_account(&subnet_account, tao_bal.into());
}

pub fn remove_owner_registration_stake(netuid: NetUid) {
    let owner_hotkey = SubnetOwnerHotkey::<Test>::get(netuid);
    let owner_coldkey = SubnetOwner::<Test>::get(netuid);

    let owner_stake = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
        &owner_hotkey,
        &owner_coldkey,
        netuid,
    );

    if owner_stake.is_zero() {
        return;
    }

    let alpha_out_before = SubnetAlphaOut::<Test>::get(netuid);

    SubtensorModule::decrease_stake_for_hotkey_and_coldkey_on_subnet(
        &owner_hotkey,
        &owner_coldkey,
        netuid,
        owner_stake,
    );

    SubnetAlphaOut::<Test>::insert(netuid, alpha_out_before.saturating_sub(owner_stake));

    assert_eq!(
        SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &owner_hotkey,
            &owner_coldkey,
            netuid,
        ),
        AlphaBalance::ZERO
    );
    assert_eq!(
        TotalHotkeyAlpha::<Test>::get(owner_hotkey, netuid),
        AlphaBalance::ZERO
    );
}

pub fn root_register_with_stake(coldkey: &U256, hotkey: &U256, alpha_netuid: NetUid) {
    register_ok_neuron(alpha_netuid, *hotkey, *coldkey, 0);
    SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
        hotkey,
        coldkey,
        NetUid::ROOT,
        AlphaBalance::from(1_000_000_000),
    );
    assert_ok!(SubtensorModule::root_register(
        RuntimeOrigin::signed(*coldkey),
        *hotkey,
    ));
}
