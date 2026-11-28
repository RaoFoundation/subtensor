#![allow(clippy::arithmetic_side_effects, clippy::expect_used)]

use frame_support::{
    PalletId, derive_impl, ord_parameter_types, parameter_types, traits::EitherOfDiverse,
};
use frame_system::{EnsureRoot, EnsureSignedBy};
use sp_runtime::{BuildStorage, Permill};
use subtensor_runtime_common::{TaoBalance, TusdBalance};

use crate::TaoUsdOracle;

type Block = frame_system::mocking::MockBlock<Test>;

frame_support::construct_runtime!(
    pub enum Test {
        System: frame_system = 1,
        Balances: pallet_balances = 2,
        Tusd: pallet_tusd = 3,
        Pool: crate = 4,
    }
);

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl frame_system::Config for Test {
    type Block = Block;
    type AccountData = pallet_balances::AccountData<TaoBalance>;
}

/// The account that may only pause.
pub const GUARDIAN: u64 = 99;
/// One TAO, or one tUSD.
pub const UNIT: u64 = 1_000_000_000;
pub const EXISTENTIAL_DEPOSIT: u64 = 500;
pub const MIN_RESERVE: u64 = 1_000_000;
pub const TWAP_PERIOD: u32 = 10;

ord_parameter_types! {
    pub const Guardian: u64 = GUARDIAN;
}

parameter_types! {
    pub const ExistentialDeposit: TaoBalance = TaoBalance::new(EXISTENTIAL_DEPOSIT);
    pub const MinBalance: TusdBalance = TusdBalance::new(10_000_000);
    pub const PoolPalletId: PalletId = PalletId(*b"sn/cpool");
    pub const MaxInitialPriceDeviation: Permill = Permill::from_percent(2);
    pub const MinReserve: u64 = MIN_RESERVE;
    pub const TwapPeriod: u32 = TWAP_PERIOD;
    /// The oracle's TAO price: 400 dollars unless a test says otherwise.
    pub static OraclePrice: Option<u64> = Some(400 * UNIT);
}

#[derive_impl(pallet_balances::config_preludes::TestDefaultConfig)]
impl pallet_balances::Config for Test {
    type AccountStore = System;
    type Balance = TaoBalance;
    type ExistentialDeposit = ExistentialDeposit;
}

pub type Pauser = EitherOfDiverse<EnsureRoot<u64>, EnsureSignedBy<Guardian, u64>>;

impl pallet_tusd::Config for Test {
    type AdminOrigin = EnsureRoot<u64>;
    type PauseOrigin = Pauser;
    type MinBalance = MinBalance;
    type WeightInfo = ();
}

pub struct MockOracle;
impl TaoUsdOracle for MockOracle {
    fn tao_usd_price() -> Option<u64> {
        OraclePrice::get()
    }
}

impl crate::Config for Test {
    type Currency = Balances;
    type AdminOrigin = EnsureRoot<u64>;
    type PauseOrigin = Pauser;
    type PriceOracle = MockOracle;
    type PalletId = PoolPalletId;
    type MaxInitialPriceDeviation = MaxInitialPriceDeviation;
    type MinReserve = MinReserve;
    type TwapPeriod = TwapPeriod;
    type WeightInfo = ();
}

pub fn new_test_ext() -> sp_io::TestExternalities {
    let storage = frame_system::GenesisConfig::<Test>::default()
        .build_storage()
        .expect("frame_system storage should build");
    let mut ext = sp_io::TestExternalities::new(storage);
    ext.execute_with(|| System::set_block_number(1));
    ext
}
