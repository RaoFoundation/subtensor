#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used)]
//! Mock runtime wiring the real `pallet-subtensor` and `pallet-subtensor-swap` under
//! `pallet-derivatives`, so tests exercise the real pool, stake and reserve accounting.

use core::num::NonZeroU64;

use frame_support::{
    PalletId, derive_impl, parameter_types,
    traits::{Everything, Hooks, PrivilegeCmp},
    weights::WeightMeter,
};
use frame_system::{self as system, EnsureRoot, limits};
use sp_core::{ConstU64, H256, U256};
use sp_runtime::{
    BuildStorage, KeyTypeId, Perbill, Percent,
    testing::TestXt,
    traits::{AccountIdConversion, BlakeTwo256, ConstU32, IdentityLookup},
};
use sp_std::cmp::Ordering;
use sp_weights::Weight;
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::{
    AlphaBalance, AuthorshipInfo, ConstTao, NetUid, SubnetDissolveHook, TaoBalance, Token,
};

use crate::{self as pallet_derivatives, Side};

type Block = frame_system::mocking::MockBlock<Test>;

frame_support::construct_runtime!(
    pub enum Test {
        System: frame_system = 1,
        Balances: pallet_balances = 2,
        Derivatives: pallet_derivatives = 3,
        SubtensorModule: pallet_subtensor::{Pallet, Call, Storage, Event<T>, Error<T>} = 4,
        Scheduler: pallet_scheduler::{Pallet, Call, Storage, Event<T>} = 5,
        Drand: pallet_drand::{Pallet, Call, Storage, Event<T>} = 6,
        Grandpa: pallet_grandpa = 7,
        EVMChainId: pallet_evm_chain_id = 8,
        Swap: pallet_subtensor_swap::{Pallet, Call, Storage, Event<T>} = 9,
        Preimage: pallet_preimage::{Pallet, Call, Storage, Event<T>} = 10,
        Crowdloan: pallet_crowdloan::{Pallet, Call, Storage, Event<T>} = 11,
        AlphaAssets: pallet_alpha_assets = 12,
    }
);

pub type AccountId = U256;
pub type Balance = TaoBalance;
pub type TestAuthId = test_crypto::TestAuthId;
pub type UncheckedExtrinsic = TestXt<RuntimeCall, ()>;
pub type TestRuntimeCall = frame_system::Call<Test>;

parameter_types! {
    pub const BlockHashCount: u64 = 250;
    pub const SS58Prefix: u8 = 42;
}

pub struct MockAuthorshipProvider;

impl AuthorshipInfo<U256> for MockAuthorshipProvider {
    fn author() -> Option<U256> {
        Some(U256::from(12345u64))
    }
}

parameter_types! {
    pub const InitialMinAllowedWeights: u16 = 0;
    pub const InitialEmissionValue: u16 = 0;
    pub BlockWeights: limits::BlockWeights = limits::BlockWeights::with_sensible_defaults(
        Weight::from_parts(2_000_000_000_000, u64::MAX),
        Perbill::from_percent(75),
    );
    pub const InitialRho: u16 = 30;
    pub const InitialAlphaSigmoidSteepness: i16 = 1000;
    pub const InitialKappa: u16 = 32_767;
    pub const InitialTempo: u16 = 0;
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
    pub const InitialWeightsVersionKey: u16 = 0;
    pub const InitialServingRateLimit: u64 = 0;
    pub const InitialTxRateLimit: u64 = 0;
    pub const InitialTxDelegateTakeRateLimit: u64 = 0;
    pub const InitialTxChildKeyTakeRateLimit: u64 = 0;
    pub const InitialBurn: TaoBalance = TaoBalance::new(0);
    pub const InitialMinBurn: TaoBalance = TaoBalance::new(500_000);
    pub const InitialMinStake: TaoBalance = TaoBalance::new(2_000_000);
    pub const InitialMinTransfer: TaoBalance = TaoBalance::new(2_000_000);
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
    pub const InitialDifficulty: u64 = 10000;
    pub const InitialActivityCutoff: u16 = 5000;
    pub const InitialAdjustmentInterval: u16 = 100;
    pub const InitialAdjustmentAlpha: u64 = 0;
    pub const InitialMaxRegistrationsPerBlock: u16 = 3;
    pub const InitialTargetRegistrationsPerInterval: u16 = 2;
    pub const InitialPruningScore : u16 = u16::MAX;
    pub const InitialMinDifficulty: u64 = 1;
    pub const InitialMaxDifficulty: u64 = u64::MAX;
    pub const InitialRAORecycledForRegistration: TaoBalance = TaoBalance::new(0);
    pub const InitialNetworkImmunityPeriod: u64 = 1_296_000;
    pub const InitialNetworkMinLockCost: TaoBalance = TaoBalance::new(100_000_000_000);
    pub const InitialSubnetOwnerCut: u16 = 0;
    pub const InitialNetworkLockReductionInterval: u64 = 2;
    pub const InitialNetworkRateLimit: u64 = 0;
    pub const InitialKeySwapCost: TaoBalance = TaoBalance::new(1_000_000_000);
    pub const InitialAlphaHigh: u16 = 58982;
    pub const InitialAlphaLow: u16 = 45875;
    pub const InitialLiquidAlphaOn: bool = false;
    pub const InitialYuma3On: bool = false;
    pub const InitialColdkeySwapAnnouncementDelay: u64 = 50;
    pub const InitialColdkeySwapReannouncementDelay: u64 = 10;
    pub const InitialDissolveNetworkScheduleDuration: u64 = 5 * 24 * 60 * 60 / 12;
    pub const InitialTaoWeight: u64 = u64::MAX/10;
    pub const InitialEmaPriceHalvingPeriod: u64 = 201_600_u64;
    pub const InitialStartCallDelay: u64 = 0;
    pub const InitialKeySwapOnSubnetCost: TaoBalance = TaoBalance::new(10_000_000);
    pub const HotkeySwapOnSubnetInterval: u64 = 7 * 24 * 60 * 60 / 12;
    pub const LeaseDividendsDistributionInterval: u32 = 100;
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
    type InitialMinTransfer = InitialMinTransfer;
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
    type Derivatives = Derivatives;
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

parameter_types! {
    pub const SwapProtocolId: PalletId = PalletId(*b"ten/swap");
    pub const SwapMaxFeeRate: u16 = 10000;
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
impl SubnetDissolveHook for CommitmentsI {
    fn on_subnet_dissolve(_netuid: NetUid, _weight_meter: &mut WeightMeter) -> bool {
        true
    }
}
impl pallet_subtensor::CommitmentsInterface<AccountId> for CommitmentsI {
    fn purge_neuron(_netuid: NetUid, _account: &AccountId) {}
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

// ── Derivatives config ───────────────────────────────────────────────────────

parameter_types! {
    pub const DerivativesPalletId: PalletId = PalletId(*b"bt/deriv");
}

impl pallet_derivatives::Config for Test {
    type Pool = SubtensorModule;
    type PalletId = DerivativesPalletId;
    type MaxExpiriesPerBlock = ConstU32<2>;
    type WeightInfo = ();
}

// ── Helpers ──────────────────────────────────────────────────────────────────

pub const TAO: u64 = 1_000_000_000;

pub fn new_test_ext() -> sp_io::TestExternalities {
    sp_tracing::try_init_simple();
    let t = frame_system::GenesisConfig::<Test>::default()
        .build_storage()
        .unwrap();
    let mut ext = sp_io::TestExternalities::new(t);
    ext.execute_with(|| {
        System::set_block_number(1);
        // Claims the pallet hotkey, as the first block after an upgrade does on chain.
        <Derivatives as frame_support::traits::OnRuntimeUpgrade>::on_runtime_upgrade();
    });
    ext
}

pub fn pallet_account() -> AccountId {
    DerivativesPalletId::get().into_account_truncating()
}

/// The hotkey `on_runtime_upgrade` claimed for the pallet in `new_test_ext`.
pub fn pallet_hotkey() -> AccountId {
    Derivatives::pallet_hotkey().expect("claimed at upgrade")
}

/// Run the derivatives `on_idle` hook at the current block with a large weight budget.
pub fn run_idle() -> Weight {
    Derivatives::on_idle(System::block_number(), Weight::MAX)
}

/// A dynamic subnet with `tao` and `alpha` in the pool and a balancer initialised at the
/// implied price.
pub fn add_dynamic_network(netuid: NetUid, tao: u64, alpha: u64) {
    SubtensorModule::init_new_network(netuid, 1);
    pallet_subtensor::SubtokenEnabled::<Test>::insert(netuid, true);
    pallet_subtensor::SubnetMechanism::<Test>::insert(netuid, 1);
    pallet_subtensor::FirstEmissionBlockNumber::<Test>::insert(netuid, 1);
    pallet_subtensor::SubnetTAO::<Test>::insert(netuid, TaoBalance::from(tao));
    pallet_subtensor::SubnetAlphaIn::<Test>::insert(netuid, AlphaBalance::from(alpha));
    pallet_subtensor::SubnetAlphaOut::<Test>::insert(netuid, AlphaBalance::from(alpha));
    pallet_subtensor::TotalStake::<Test>::mutate(|t| *t = t.saturating_add(TaoBalance::from(tao)));
    // The subnet sub-account must physically hold the TAO reserve.
    let subnet_account = SubtensorModule::get_subnet_account_id(netuid).unwrap();
    add_balance(&subnet_account, tao);
    let price = U64F64::from_num(tao) / U64F64::from_num(alpha);
    <Swap as subtensor_swap_interface::SwapHandler>::init_swap(netuid, Some(price));
}

pub fn add_balance(who: &AccountId, tao: u64) {
    let credit = SubtensorModule::mint_tao(TaoBalance::from(tao));
    let remainder = SubtensorModule::spend_tao(who, credit, TaoBalance::from(tao))
        .unwrap_or_else(|_| panic!("mint failed"));
    drop(remainder);
}

pub fn balance(who: &AccountId) -> u64 {
    SubtensorModule::get_coldkey_balance(who).into()
}

pub fn stake(coldkey: &AccountId, hotkey: &AccountId, netuid: NetUid) -> u64 {
    SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(hotkey, coldkey, netuid).into()
}

/// Give `coldkey` alpha stake at `hotkey` without going through the pool.
pub fn give_stake(coldkey: &AccountId, hotkey: &AccountId, netuid: NetUid, alpha: u64) {
    let _ = SubtensorModule::create_account_if_non_existent(coldkey, hotkey);
    SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
        hotkey,
        coldkey,
        netuid,
        AlphaBalance::from(alpha),
    );
    pallet_subtensor::SubnetAlphaOut::<Test>::mutate(netuid, |v| {
        *v = v.saturating_add(AlphaBalance::from(alpha))
    });
}

pub fn reserves(netuid: NetUid) -> (u64, u64) {
    (
        pallet_subtensor::SubnetTAO::<Test>::get(netuid).into(),
        pallet_subtensor::SubnetAlphaIn::<Test>::get(netuid).into(),
    )
}

pub fn alpha_out(netuid: NetUid) -> u64 {
    pallet_subtensor::SubnetAlphaOut::<Test>::get(netuid).into()
}

pub fn total_stake() -> u64 {
    pallet_subtensor::TotalStake::<Test>::get().into()
}

pub fn tao_flow(netuid: NetUid) -> i64 {
    pallet_subtensor::SubnetTaoFlow::<Test>::get(netuid)
}

pub fn price(netuid: NetUid) -> U64F64 {
    <Swap as subtensor_swap_interface::SwapHandler>::current_alpha_price(netuid)
}

pub fn balancer_weight(netuid: NetUid) -> sp_runtime::Perquintill {
    pallet_subtensor_swap::SwapBalancer::<Test>::get(netuid).get_quote_weight()
}

pub fn position(owner: &AccountId, netuid: NetUid, side: Side) -> Option<crate::Position<u64>> {
    crate::Positions::<Test>::get(owner, (netuid, side))
}

/// Run the dissolution hook for `netuid` until it reports done.
pub fn settle_all_for_dissolution(netuid: NetUid) {
    let mut meter = WeightMeter::with_limit(Weight::MAX);
    assert!(<Derivatives as SubnetDissolveHook>::on_subnet_dissolve(
        netuid, &mut meter
    ));
}
