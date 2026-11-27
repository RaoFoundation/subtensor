#![allow(clippy::arithmetic_side_effects, clippy::expect_used)]

use frame_support::{derive_impl, ord_parameter_types, parameter_types, traits::EitherOfDiverse};
use frame_system::{EnsureRoot, EnsureSignedBy};
use sp_runtime::BuildStorage;
use subtensor_runtime_common::TusdBalance;

type Block = frame_system::mocking::MockBlock<Test>;

frame_support::construct_runtime!(
    pub enum Test {
        System: frame_system = 1,
        Tusd: crate = 2,
    }
);

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl frame_system::Config for Test {
    type Block = Block;
}

/// The account that may only disable bridges.
pub const GUARDIAN: u64 = 99;
/// 0.01 tUSD.
pub const MIN_BALANCE: u64 = 10_000_000;

ord_parameter_types! {
    pub const Guardian: u64 = GUARDIAN;
}

parameter_types! {
    pub const MinBalance: TusdBalance = TusdBalance::new(MIN_BALANCE);
}

impl crate::Config for Test {
    type AdminOrigin = EnsureRoot<u64>;
    type PauseOrigin = EitherOfDiverse<EnsureRoot<u64>, EnsureSignedBy<Guardian, u64>>;
    type MinBalance = MinBalance;
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
