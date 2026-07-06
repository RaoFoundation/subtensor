#![cfg(test)]
#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::unwrap_used
)]
use frame_support::{
    PalletId, derive_impl, parameter_types,
    traits::{OnFinalize, OnInitialize, fungible, fungible::*, tokens::Preservation},
    weights::Weight,
};
use frame_system::{EnsureRoot, pallet_prelude::BlockNumberFor};
use sp_core::U256;
use sp_runtime::{BuildStorage, traits::IdentityLookup};
use subtensor_runtime_common::TaoBalance;

use crate::{BalanceOf, CrowdloanId, pallet as pallet_crowdloan, weights::WeightInfo};

type Block = frame_system::mocking::MockBlock<Test>;
pub(crate) type AccountOf<T> = <T as frame_system::Config>::AccountId;

frame_support::construct_runtime!(
    pub enum Test
    {
      System: frame_system = 1,
      Balances: pallet_balances = 2,
      Crowdloan: pallet_crowdloan = 3,
      Preimage: pallet_preimage = 4,
      TestPallet: pallet_test = 5,
    }
);

#[allow(unused)]
pub(crate) fn new_test_ext() -> sp_io::TestExternalities {
    let mut t = frame_system::GenesisConfig::<Test>::default()
        .build_storage()
        .expect("Expected to not panic");
    pallet_balances::GenesisConfig::<Test> {
        balances: vec![
            (U256::from(1), 10.into()),
            (U256::from(2), 10.into()),
            (U256::from(3), 10.into()),
            (U256::from(4), 10.into()),
            (U256::from(5), 3.into()),
        ],
        dev_accounts: None,
    }
    .assimilate_storage(&mut t)
    .expect("Expected to not panic");
    let mut ext = sp_io::TestExternalities::new(t);
    ext.execute_with(|| System::set_block_number(1));
    ext
}

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl frame_system::Config for Test {
    type Block = Block;
    type AccountId = U256;
    type AccountData = pallet_balances::AccountData<TaoBalance>;
    type Lookup = IdentityLookup<Self::AccountId>;
}

parameter_types! {
    pub const ExistentialDeposit: TaoBalance = TaoBalance::new(1);
}

#[derive_impl(pallet_balances::config_preludes::TestDefaultConfig)]
impl pallet_balances::Config for Test {
    type AccountStore = System;
    type Balance = TaoBalance;
    type ExistentialDeposit = ExistentialDeposit;
}

pub struct TestWeightInfo;
impl WeightInfo for TestWeightInfo {
    fn create() -> Weight {
        Weight::zero()
    }
    fn contribute() -> Weight {
        Weight::zero()
    }
    fn withdraw() -> Weight {
        Weight::zero()
    }
    fn refund(_k: u32) -> Weight {
        Weight::zero()
    }
    fn finalize() -> Weight {
        Weight::zero()
    }
    fn dissolve() -> Weight {
        Weight::zero()
    }
    fn update_min_contribution() -> Weight {
        Weight::zero()
    }
    fn update_end() -> Weight {
        Weight::zero()
    }
    fn update_cap() -> Weight {
        Weight::zero()
    }
    fn set_max_contribution() -> Weight {
        Weight::zero()
    }
}

parameter_types! {
    pub const PreimageMaxSize: u32 = 4096 * 1024;
    pub const PreimageBaseDeposit: TaoBalance = TaoBalance::new(1);
    pub const PreimageByteDeposit: TaoBalance = TaoBalance::new(1);
}

impl pallet_preimage::Config for Test {
    type WeightInfo = pallet_preimage::weights::SubstrateWeight<Test>;
    type RuntimeEvent = RuntimeEvent;
    type Currency = Balances;
    type ManagerOrigin = EnsureRoot<AccountOf<Test>>;
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
    type WeightInfo = TestWeightInfo;
    type Preimages = Preimage;
    type MinimumDeposit = MinimumDeposit;
    type AbsoluteMinimumContribution = AbsoluteMinimumContribution;
    type MinimumBlockDuration = MinimumBlockDuration;
    type MaximumBlockDuration = MaximumBlockDuration;
    type RefundContributorsLimit = RefundContributorsLimit;
    type MaxContributors = MaxContributors;
}

// A test pallet used to test some behavior of the crowdloan pallet
#[allow(unused)]
#[frame_support::pallet(dev_mode)]
pub(crate) mod pallet_test {
    use super::*;
    use frame_support::{
        dispatch::DispatchResult,
        pallet_prelude::{OptionQuery, StorageValue},
    };
    use frame_system::pallet_prelude::OriginFor;
    use subtensor_runtime_common::TaoBalance;

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    #[pallet::config]
    pub trait Config: frame_system::Config + pallet_crowdloan::Config {
        type Currency: fungible::Balanced<Self::AccountId, Balance = TaoBalance>
            + fungible::Mutate<Self::AccountId>;
    }

    #[pallet::error]
    pub enum Error<T> {
        ShouldFail,
        MissingCurrentCrowdloanId,
        CrowdloanDoesNotExist,
    }

    #[pallet::storage]
    pub type PassedCrowdloanId<T: Config> = StorageValue<_, CrowdloanId, OptionQuery>;

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        #[pallet::call_index(0)]
        pub fn noop(origin: OriginFor<T>) -> DispatchResult {
            Ok(())
        }

        #[pallet::call_index(1)]
        pub fn transfer_funds(origin: OriginFor<T>, dest: AccountOf<T>) -> DispatchResult {
            let crowdloan_id = pallet_crowdloan::CurrentCrowdloanId::<T>::get()
                .ok_or(Error::<T>::MissingCurrentCrowdloanId)?;
            let crowdloan = pallet_crowdloan::Crowdloans::<T>::get(crowdloan_id)
                .ok_or(Error::<T>::CrowdloanDoesNotExist)?;

            PassedCrowdloanId::<T>::put(crowdloan_id);

            <T as Config>::Currency::transfer(
                &crowdloan.funds_account,
                &dest,
                crowdloan.raised,
                Preservation::Expendable,
            )?;

            Ok(())
        }

        #[pallet::call_index(2)]
        pub fn set_passed_crowdloan_id(origin: OriginFor<T>) -> DispatchResult {
            let crowdloan_id = pallet_crowdloan::CurrentCrowdloanId::<T>::get()
                .ok_or(Error::<T>::MissingCurrentCrowdloanId)?;

            PassedCrowdloanId::<T>::put(crowdloan_id);

            Ok(())
        }

        #[pallet::call_index(3)]
        pub fn failing_extrinsic(origin: OriginFor<T>) -> DispatchResult {
            Err(Error::<T>::ShouldFail.into())
        }
    }
}

impl pallet_test::Config for Test {
    type Currency = Balances;
}

pub(crate) struct TestState {
    block_number: BlockNumberFor<Test>,
    balances: Vec<(AccountOf<Test>, BalanceOf<Test>)>,
}

impl Default for TestState {
    fn default() -> Self {
        Self {
            block_number: 1,
            balances: vec![],
        }
    }
}

impl TestState {
    pub(crate) fn with_block_number(mut self, block_number: BlockNumberFor<Test>) -> Self {
        self.block_number = block_number;
        self
    }

    pub(crate) fn with_balance(mut self, who: AccountOf<Test>, balance: BalanceOf<Test>) -> Self {
        self.balances.push((who, balance));
        self
    }

    pub(crate) fn build_and_execute(self, test: impl FnOnce()) {
        let mut t = frame_system::GenesisConfig::<Test>::default()
            .build_storage()
            .unwrap();

        pallet_balances::GenesisConfig::<Test> {
            balances: self
                .balances
                .iter()
                .map(|(who, balance)| (*who, *balance))
                .collect::<Vec<_>>(),
            dev_accounts: None,
        }
        .assimilate_storage(&mut t)
        .unwrap();

        let mut ext = sp_io::TestExternalities::new(t);
        ext.execute_with(|| System::set_block_number(self.block_number));
        ext.execute_with(test);
    }
}

pub(crate) fn last_event() -> RuntimeEvent {
    System::events().pop().expect("RuntimeEvent expected").event
}

pub(crate) fn run_to_block(n: u64) {
    while System::block_number() < n {
        System::on_finalize(System::block_number());
        Balances::on_finalize(System::block_number());
        System::reset_events();
        System::set_block_number(System::block_number() + 1);
        Balances::on_initialize(System::block_number());
        System::on_initialize(System::block_number());
    }
}

pub(crate) fn noop_call() -> Box<RuntimeCall> {
    Box::new(RuntimeCall::TestPallet(pallet_test::Call::<Test>::noop {}))
}
