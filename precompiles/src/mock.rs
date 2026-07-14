#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]
#![allow(clippy::arithmetic_side_effects)]

use core::{marker::PhantomData, num::NonZeroU64};

use fp_evm::{Context, PrecompileResult};
use frame_support::{
    PalletId, derive_impl, parameter_types,
    traits::{Everything, PrivilegeCmp},
    weights::Weight,
};
use frame_system::{EnsureRoot, limits};
use pallet_evm::{
    AddressMapping, BalanceConverter, EnsureAddressNever, EnsureAddressRoot, EvmBalance,
    PrecompileHandle, PrecompileSet, SubstrateBalance,
};
use precompile_utils::testing::MockHandle;
use sp_core::{ConstU64, H160, H256, U256, crypto::AccountId32};
use sp_runtime::{
    BuildStorage, KeyTypeId, Perbill, Percent,
    testing::TestXt,
    traits::{BlakeTwo256, ConstU32, IdentityLookup},
};
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::{AuthorshipInfo, NetUid, ProxyType, TaoBalance};

use crate::PrecompileExt;

pub(crate) type AccountId = AccountId32;
pub(crate) type Block = frame_system::mocking::MockBlock<Runtime>;
pub(crate) type UncheckedExtrinsic = TestXt<RuntimeCall, ()>;

frame_support::construct_runtime!(
    pub enum Runtime {
        System: frame_system = 1,
        Balances: pallet_balances = 2,
        AlphaAssets: pallet_alpha_assets = 15,
        Timestamp: pallet_timestamp = 3,
        Shield: pallet_shield = 4,
        SubtensorModule: pallet_subtensor::{Pallet, Call, Storage, Event<T>} = 5,
        Scheduler: pallet_scheduler::{Pallet, Call, Storage, Event<T>} = 6,
        Preimage: pallet_preimage::{Pallet, Call, Storage, Event<T>} = 7,
        Drand: pallet_drand::{Pallet, Call, Storage, Event<T>} = 8,
        Swap: pallet_subtensor_swap::{Pallet, Call, Storage, Event<T>} = 9,
        Crowdloan: pallet_crowdloan::{Pallet, Call, Storage, Event<T>} = 10,
        Proxy: pallet_subtensor_proxy = 11,
        Evm: pallet_evm = 12,
        AdminUtils: pallet_admin_utils = 13,
        EVMChainId: pallet_evm_chain_id = 14,
    }
);

const EVM_DECIMALS_FACTOR: u64 = 1_000_000_000;

parameter_types! {
    pub const BlockHashCount: u64 = 250;
    pub const SS58Prefix: u8 = 42;
    pub BlockWeights: limits::BlockWeights = limits::BlockWeights::with_sensible_defaults(
        Weight::from_parts(2_000_000_000_000, u64::MAX),
        Perbill::from_percent(75),
    );
    pub const ExistentialDeposit: TaoBalance = TaoBalance::new(1);
    pub const MinimumPeriod: u64 = 5;
    pub const PreimageMaxSize: u32 = 4096 * 1024;
    pub const PreimageBaseDeposit: TaoBalance = TaoBalance::new(1);
    pub const PreimageByteDeposit: TaoBalance = TaoBalance::new(1);
    pub const CrowdloanPalletId: PalletId = PalletId(*b"bt/cloan");
    pub const MinimumDeposit: TaoBalance = TaoBalance::new(50);
    pub const AbsoluteMinimumContribution: TaoBalance = TaoBalance::new(10);
    pub const MinimumBlockDuration: u64 = 20;
    pub const MaximumBlockDuration: u64 = 100;
    pub const RefundContributorsLimit: u32 = 5;
    pub const MaxContributors: u32 = 10;
    pub const SwapProtocolId: PalletId = PalletId(*b"ten/swap");
    pub const SwapMaxFeeRate: u16 = 10000;
    pub const SwapMinimumLiquidity: u64 = 1_000;
    pub const SwapMinimumReserve: NonZeroU64 = NonZeroU64::new(1_000_000).unwrap();
    pub MaximumSchedulerWeight: Weight = Perbill::from_percent(80) *
        BlockWeights::get().max_block;
    pub const MaxScheduledPerBlock: u32 = 50;
    pub const MaxAuthorities: u32 = 32;
    pub static BlockGasLimit: U256 = U256::max_value();
    pub WeightPerGas: Weight = Weight::from_parts(20_000, 0);
    pub const ProxyDepositBase: TaoBalance = TaoBalance::new(1);
    pub const ProxyDepositFactor: TaoBalance = TaoBalance::new(1);
    pub const MaxProxies: u32 = 20;
    pub const MaxPending: u32 = 15;
    pub const AnnouncementDepositBase: TaoBalance = TaoBalance::new(1);
    pub const AnnouncementDepositFactor: TaoBalance = TaoBalance::new(1);
    pub const InitialMinAllowedWeights: u16 = 0;
    pub const InitialEmissionValue: u16 = 0;
    pub const InitialRho: u16 = 30;
    pub const InitialAlphaSigmoidSteepness: i16 = 1000;
    pub const InitialKappa: u16 = 32_767;
    pub const InitialTempo: u16 = 360;
    pub const InitialImmunityPeriod: u16 = 2;
    pub const InitialMinAllowedUids: u16 = 2;
    pub const InitialMaxAllowedUids: u16 = 256;
    pub const InitialBondsMovingAverage: u64 = 900_000;
    pub const InitialBondsPenalty: u16 = u16::MAX;
    pub const InitialBondsResetOn: bool = false;
    pub const InitialDefaultDelegateTake: u16 = 11_796;
    pub const InitialMinDelegateTake: u16 = 5_898;
    pub const InitialDefaultChildKeyTake: u16 = 0;
    pub const InitialMinChildKeyTake: u16 = 0;
    pub const InitialMaxChildKeyTake: u16 = 11_796;
    pub const InitialWeightsVersionKey: u64 = 0;
    pub const InitialServingRateLimit: u64 = 0;
    pub const InitialTxRateLimit: u64 = 0;
    pub const InitialTxDelegateTakeRateLimit: u64 = 0;
    pub const InitialTxChildKeyTakeRateLimit: u64 = 0;
    pub const InitialBurn: TaoBalance = TaoBalance::new(0);
    pub const InitialMinBurn: TaoBalance = TaoBalance::new(500_000);
    pub const InitialMinStake: TaoBalance = TaoBalance::new(2_000_000);
    pub const InitialMaxBurn: TaoBalance = TaoBalance::new(1_000_000_000);
    pub const MinBurnUpperBound: TaoBalance = TaoBalance::new(1_000_000_000);
    pub const MaxBurnLowerBound: TaoBalance = TaoBalance::new(100_000_000);
    pub const MinTempo: u16 = pallet_subtensor::MIN_TEMPO;
    pub const MaxTempo: u16 = pallet_subtensor::MAX_TEMPO;
    pub const MinActivityCutoffFactorMilli: u32 = pallet_subtensor::MIN_ACTIVITY_CUTOFF_FACTOR_MILLI;
    pub const MaxActivityCutoffFactorMilli: u32 = pallet_subtensor::MAX_ACTIVITY_CUTOFF_FACTOR_MILLI;
    pub const InitialValidatorPruneLen: u64 = 0;
    pub const InitialScalingLawPower: u16 = 50;
    pub const InitialMaxAllowedValidators: u16 = 100;
    pub const InitialIssuance: TaoBalance = TaoBalance::new(0);
    pub const InitialDifficulty: u64 = 10_000;
    pub const InitialActivityCutoff: u16 = 5_000;
    pub const InitialAdjustmentInterval: u16 = 100;
    pub const InitialAdjustmentAlpha: u64 = 0;
    pub const InitialMaxRegistrationsPerBlock: u16 = 3;
    pub const InitialTargetRegistrationsPerInterval: u16 = 2;
    pub const InitialPruningScore: u16 = u16::MAX;
    pub const InitialMinDifficulty: u64 = 1;
    pub const InitialMaxDifficulty: u64 = u64::MAX;
    pub const InitialRAORecycledForRegistration: TaoBalance = TaoBalance::new(0);
    pub const InitialNetworkImmunityPeriod: u64 = 1_296_000;
    pub const InitialNetworkMinLockCost: TaoBalance = TaoBalance::new(100_000_000_000);
    pub const InitialSubnetOwnerCut: u16 = 0;
    pub const InitialNetworkLockReductionInterval: u64 = 2;
    pub const InitialNetworkRateLimit: u64 = 0;
    pub const InitialKeySwapCost: TaoBalance = TaoBalance::new(1_000_000_000);
    pub const InitialAlphaHigh: u16 = 58_982;
    pub const InitialAlphaLow: u16 = 45_875;
    pub const InitialLiquidAlphaOn: bool = false;
    pub const InitialYuma3On: bool = false;
    pub const InitialColdkeySwapAnnouncementDelay: u64 = 50;
    pub const InitialColdkeySwapReannouncementDelay: u64 = 10;
    pub const InitialDissolveNetworkScheduleDuration: u64 = 36_000;
    pub const InitialTaoWeight: u64 = u64::MAX / 10;
    pub const InitialEmaPriceHalvingPeriod: u64 = 201_600;
    pub const InitialStartCallDelay: u64 = 0;
    pub const InitialKeySwapOnSubnetCost: TaoBalance = TaoBalance::new(10_000_000);
    pub const HotkeySwapOnSubnetInterval: u64 = 50_400;
    pub const LeaseDividendsDistributionInterval: u32 = 100;
    pub const MaxImmuneUidsPercentage: Percent = Percent::from_percent(80);
    pub const EvmKeyAssociateRateLimit: u64 = 0;
    pub const SubtensorPalletId: PalletId = PalletId(*b"subtensr");
    pub const BurnAccountId: PalletId = PalletId(*b"burntnsr");
    pub const MaxEpochsPerBlock: u8 = 32;
}

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl frame_system::Config for Runtime {
    type BaseCallFilter = Everything;
    type BlockWeights = BlockWeights;
    type RuntimeOrigin = RuntimeOrigin;
    type RuntimeCall = RuntimeCall;
    type Hash = H256;
    type Hashing = BlakeTwo256;
    type AccountId = AccountId;
    type Lookup = IdentityLookup<Self::AccountId>;
    type RuntimeEvent = RuntimeEvent;
    type BlockHashCount = BlockHashCount;
    type PalletInfo = PalletInfo;
    type AccountData = pallet_balances::AccountData<TaoBalance>;
    type SS58Prefix = SS58Prefix;
    type MaxConsumers = ConstU32<16>;
    type Block = Block;
    type Nonce = u64;
    type DispatchExtension = (
        pallet_subtensor::CheckColdkeySwap<Runtime>,
        pallet_subtensor::CheckWeights<Runtime>,
        pallet_subtensor::CheckRateLimits<Runtime>,
        pallet_subtensor::CheckDelegateTake<Runtime>,
        pallet_subtensor::CheckServingEndpoints<Runtime>,
        pallet_subtensor::CheckEvmKeyAssociation<Runtime>,
    );
}

#[derive_impl(pallet_balances::config_preludes::TestDefaultConfig)]
impl pallet_balances::Config for Runtime {
    type Balance = TaoBalance;
    type RuntimeEvent = RuntimeEvent;
    type ExistentialDeposit = ExistentialDeposit;
    type AccountStore = System;
    type ReserveIdentifier = [u8; 8];
    type FreezeIdentifier = ();
    type MaxFreezes = ();
    type RuntimeHoldReason = ();
}

impl pallet_alpha_assets::Config for Runtime {}

#[derive_impl(pallet_timestamp::config_preludes::TestDefaultConfig)]
impl pallet_timestamp::Config for Runtime {
    type MinimumPeriod = MinimumPeriod;
}

impl pallet_shield::Config for Runtime {
    type AuthorityId = sp_core::sr25519::Public;
    type FindAuthors = ();
    type RuntimeCall = RuntimeCall;
    type ExtrinsicDecryptor = ();
    type WeightInfo = ();
}

impl pallet_preimage::Config for Runtime {
    type WeightInfo = pallet_preimage::weights::SubstrateWeight<Runtime>;
    type RuntimeEvent = RuntimeEvent;
    type Currency = Balances;
    type ManagerOrigin = EnsureRoot<AccountId>;
    type Consideration = ();
}

pub struct FixedGasPrice;
impl pallet_evm::FeeCalculator for FixedGasPrice {
    fn min_gas_price() -> (U256, Weight) {
        (1_000_000_000u128.into(), Weight::from_parts(7, 0))
    }
}

pub struct SubtensorEvmBalanceConverter;
impl BalanceConverter for SubtensorEvmBalanceConverter {
    fn into_evm_balance(value: SubstrateBalance) -> Option<EvmBalance> {
        value
            .into_u256()
            .checked_mul(U256::from(EVM_DECIMALS_FACTOR))
            .and_then(|evm_value| (evm_value <= U256::MAX).then(|| EvmBalance::new(evm_value)))
    }

    fn into_substrate_balance(value: EvmBalance) -> Option<SubstrateBalance> {
        value
            .into_u256()
            .checked_div(U256::from(EVM_DECIMALS_FACTOR))
            .and_then(|substrate_value| {
                (substrate_value <= U256::from(u64::MAX))
                    .then(|| SubstrateBalance::new(substrate_value))
            })
    }
}

impl pallet_evm::Config for Runtime {
    type BalanceConverter = SubtensorEvmBalanceConverter;
    type AccountProvider = pallet_evm::FrameSystemAccountProvider<Self>;
    type FeeCalculator = FixedGasPrice;
    type GasWeightMapping = pallet_evm::FixedGasWeightMapping<Self>;
    type WeightPerGas = WeightPerGas;
    type BlockHashMapping = pallet_evm::SubstrateBlockHashMapping<Self>;
    type CallOrigin = EnsureAddressRoot<AccountId>;
    type WithdrawOrigin = EnsureAddressNever<AccountId>;
    type AddressMapping = pallet_evm::HashedAddressMapping<BlakeTwo256>;
    type Currency = Balances;
    type PrecompilesType = ();
    type PrecompilesValue = ();
    type ChainId = ();
    type BlockGasLimit = BlockGasLimit;
    type Runner = pallet_evm::runner::stack::Runner<Self>;
    type OnChargeTransaction = ();
    type OnCreate = ();
    type FindAuthor = ();
    type GasLimitPovSizeRatio = ();
    type GasLimitStorageGrowthRatio = ();
    type Timestamp = Timestamp;
    type CreateInnerOriginFilter = ();
    type CreateOriginFilter = ();
    type WeightInfo = pallet_evm::weights::SubstrateWeight<Runtime>;
}

impl pallet_crowdloan::Config for Runtime {
    type PalletId = CrowdloanPalletId;
    type Currency = Balances;
    type RuntimeCall = RuntimeCall;
    type WeightInfo = pallet_crowdloan::weights::SubstrateWeight<Runtime>;
    type Preimages = Preimage;
    type MinimumDeposit = MinimumDeposit;
    type AbsoluteMinimumContribution = AbsoluteMinimumContribution;
    type MinimumBlockDuration = MinimumBlockDuration;
    type MaximumBlockDuration = MaximumBlockDuration;
    type RefundContributorsLimit = RefundContributorsLimit;
    type MaxContributors = MaxContributors;
}

impl pallet_subtensor_swap::Config for Runtime {
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
    fn cmp_privilege(_left: &OriginCaller, _right: &OriginCaller) -> Option<core::cmp::Ordering> {
        None
    }
}

impl pallet_scheduler::Config for Runtime {
    type RuntimeOrigin = RuntimeOrigin;
    type RuntimeEvent = RuntimeEvent;
    type PalletsOrigin = OriginCaller;
    type RuntimeCall = RuntimeCall;
    type MaximumWeight = MaximumSchedulerWeight;
    type ScheduleOrigin = EnsureRoot<AccountId>;
    type MaxScheduledPerBlock = MaxScheduledPerBlock;
    type WeightInfo = pallet_scheduler::weights::SubstrateWeight<Runtime>;
    type OriginPrivilegeCmp = OriginPrivilegeCmp;
    type Preimages = Preimage;
    type BlockNumberProvider = System;
}

pub const KEY_TYPE: KeyTypeId = KeyTypeId(*b"test");

mod test_crypto {
    use super::{AccountId, KEY_TYPE};
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
        type AccountId = AccountId;

        fn into_account(self) -> AccountId {
            let mut bytes = [0u8; 32];
            bytes.copy_from_slice(self.as_ref());
            AccountId::new(bytes)
        }
    }
}

impl pallet_evm_chain_id::Config for Runtime {}

impl pallet_admin_utils::Config for Runtime {
    type Aura = ();
    type Grandpa = ();
    type AuthorityId = test_crypto::Public;
    type MaxAuthorities = MaxAuthorities;
    type Balance = TaoBalance;
    type WeightInfo = ();
}

impl pallet_drand::Config for Runtime {
    type AuthorityId = test_crypto::TestAuthId;
    type Verifier = pallet_drand::verifier::QuicknetVerifier;
    type UnsignedPriority = ConstU64<{ 1 << 20 }>;
    type HttpFetchTimeout = ConstU64<1_000>;
    type WeightInfo = ();
}

impl frame_system::offchain::SigningTypes for Runtime {
    type Public = test_crypto::Public;
    type Signature = test_crypto::Signature;
}

impl<LocalCall> frame_system::offchain::CreateTransactionBase<LocalCall> for Runtime
where
    RuntimeCall: From<LocalCall>,
{
    type Extrinsic = UncheckedExtrinsic;
    type RuntimeCall = RuntimeCall;
}

impl<LocalCall> frame_system::offchain::CreateBare<LocalCall> for Runtime
where
    RuntimeCall: From<LocalCall>,
{
    fn create_bare(call: Self::RuntimeCall) -> Self::Extrinsic {
        UncheckedExtrinsic::new_bare(call)
    }
}

pub struct MockAuthorshipProvider;
impl AuthorshipInfo<AccountId> for MockAuthorshipProvider {
    fn author() -> Option<AccountId> {
        Some(AccountId::new([1; 32]))
    }
}

pub struct CommitmentsI;
impl pallet_subtensor::CommitmentsInterface for CommitmentsI {
    fn purge_netuid(_netuid: NetUid) {}
}

impl pallet_subtensor::Config for Runtime {
    type RuntimeCall = RuntimeCall;
    type Currency = Balances;
    type InitialIssuance = InitialIssuance;
    type SudoRuntimeCall = frame_system::Call<Runtime>;
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
    type Preimages = Preimage;
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
    type OnRootRegistrationChange = ();
    type RootRegisteredInspector = ();
    type EmaValueProvider = ();
    type SubtensorPalletId = SubtensorPalletId;
    type BurnAccountId = BurnAccountId;
    type InitialMaxEpochsPerBlock = MaxEpochsPerBlock;
    type WeightInfo = ();
}

impl frame_support::traits::InstanceFilter<RuntimeCall> for ProxyType {
    fn filter(&self, _c: &RuntimeCall) -> bool {
        true
    }

    fn is_superset(&self, o: &Self) -> bool {
        match (self, o) {
            (x, y) if x == y => true,
            (ProxyType::Any, _) => true,
            _ => false,
        }
    }
}

impl pallet_subtensor_proxy::Config for Runtime {
    type RuntimeCall = RuntimeCall;
    type Currency = Balances;
    type ProxyType = ProxyType;
    type ProxyDepositBase = ProxyDepositBase;
    type ProxyDepositFactor = ProxyDepositFactor;
    type MaxProxies = MaxProxies;
    type WeightInfo = pallet_subtensor_proxy::weights::SubstrateWeight<Runtime>;
    type MaxPending = MaxPending;
    type CallHasher = BlakeTwo256;
    type AnnouncementDepositBase = AnnouncementDepositBase;
    type AnnouncementDepositFactor = AnnouncementDepositFactor;
    type BlockNumberProvider = System;
}

pub(crate) struct SinglePrecompileSet<P>(PhantomData<P>);

impl<P> Default for SinglePrecompileSet<P> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<P> PrecompileSet for SinglePrecompileSet<P>
where
    P: pallet_evm::Precompile + PrecompileExt<AccountId>,
{
    fn execute(&self, handle: &mut impl PrecompileHandle) -> Option<PrecompileResult> {
        (handle.code_address() == H160::from_low_u64_be(P::INDEX)).then(|| P::execute(handle))
    }

    fn is_precompile(&self, address: H160, _gas: u64) -> pallet_evm::IsPrecompileResult {
        pallet_evm::IsPrecompileResult::Answer {
            is_precompile: address == H160::from_low_u64_be(P::INDEX),
            extra_cost: 0,
        }
    }
}

pub(crate) fn precompiles<P>() -> SinglePrecompileSet<P>
where
    P: pallet_evm::Precompile + PrecompileExt<AccountId>,
{
    SinglePrecompileSet::default()
}

pub(crate) fn new_test_ext() -> sp_io::TestExternalities {
    let mut ext: sp_io::TestExternalities = RuntimeGenesisConfig::default()
        .build_storage()
        .unwrap()
        .into();
    ext.execute_with(|| System::set_block_number(1));
    ext
}

pub(crate) fn execute_precompile<PSet: PrecompileSet>(
    precompiles: &PSet,
    precompile_address: H160,
    caller: H160,
    input: Vec<u8>,
    apparent_value: U256,
) -> Option<PrecompileResult> {
    let mut handle = MockHandle::new(
        precompile_address,
        Context {
            address: precompile_address,
            caller,
            apparent_value,
        },
    );
    handle.input = input;
    precompiles.execute(&mut handle)
}

pub(crate) fn addr_from_index(index: u64) -> H160 {
    H160::from_low_u64_be(index)
}

pub(crate) fn mapped_account(address: H160) -> AccountId {
    <Runtime as pallet_evm::Config>::AddressMapping::into_account_id(address)
}

pub(crate) fn fund_account(account: &AccountId, amount: u64) {
    let amount = TaoBalance::from(amount);
    let credit = pallet_subtensor::Pallet::<Runtime>::mint_tao(amount);
    let _ = pallet_subtensor::Pallet::<Runtime>::spend_tao(account, credit, amount)
        .expect("test account funding should work");
}

pub(crate) fn abi_word(value: U256) -> Vec<u8> {
    value.to_big_endian().to_vec()
}

pub(crate) fn assert_static_call<PSet: PrecompileSet>(
    precompiles: &PSet,
    caller: H160,
    precompile_addr: H160,
    input: Vec<u8>,
    expected: U256,
) {
    use precompile_utils::testing::PrecompileTesterExt;

    precompiles
        .prepare_test(caller, precompile_addr, input)
        .with_static_call(true)
        .execute_returns_raw(abi_word(expected));
}

pub(crate) fn selector_u32(signature: &str) -> u32 {
    let hash = sp_io::hashing::keccak_256(signature.as_bytes());
    u32::from_be_bytes([hash[0], hash[1], hash[2], hash[3]])
}

pub(crate) fn alpha_price_to_evm(price: U64F64) -> U256 {
    let scaled_price = (price * U64F64::from_num(EVM_DECIMALS_FACTOR)).to_num::<u64>();
    <Runtime as pallet_evm::Config>::BalanceConverter::into_evm_balance(scaled_price.into())
        .expect("runtime balance conversion should work for alpha price")
        .into_u256()
}

pub(crate) fn substrate_to_evm(amount: u64) -> U256 {
    <Runtime as pallet_evm::Config>::BalanceConverter::into_evm_balance(amount.into())
        .expect("runtime balance conversion should work")
        .into_u256()
}
