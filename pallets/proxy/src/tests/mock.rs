//! Test runtime and helpers for the proxy pallet.

#![allow(
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::indexing_slicing
)]

use crate as proxy;
use crate::*;
use alloc::{vec, vec::Vec};
use frame::testing_prelude::*;
use frame_system::Call as SystemCall;
use pallet_balances::Call as BalancesCall;
use pallet_subtensor_utility as pallet_utility;

type Block = frame_system::mocking::MockBlock<Test>;

construct_runtime!(
    pub enum Test {
        System: frame_system = 1,
        Balances: pallet_balances = 2,
        Proxy: proxy = 3,
        Utility: pallet_utility = 4,
    }
);

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl frame_system::Config for Test {
    type Block = Block;
    type BaseCallFilter = BaseFilter;
    type AccountData = pallet_balances::AccountData<u64>;
}

#[derive_impl(pallet_balances::config_preludes::TestDefaultConfig)]
impl pallet_balances::Config for Test {
    type ReserveIdentifier = [u8; 8];
    type AccountStore = System;
}

impl pallet_utility::Config for Test {
    type RuntimeCall = RuntimeCall;
    type PalletsOrigin = OriginCaller;
    type WeightInfo = ();
}

/// Proxy permission kinds used by unit tests (`Any` is the most permissive / default).
#[derive(
    Copy,
    Clone,
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    Encode,
    Decode,
    DecodeWithMemTracking,
    RuntimeDebug,
    MaxEncodedLen,
    scale_info::TypeInfo,
)]
pub enum ProxyType {
    Any,
    JustTransfer,
    JustUtility,
}
impl Default for ProxyType {
    fn default() -> Self {
        Self::Any
    }
}
impl frame::traits::InstanceFilter<RuntimeCall> for ProxyType {
    fn filter(&self, c: &RuntimeCall) -> bool {
        match self {
            ProxyType::Any => true,
            ProxyType::JustTransfer => {
                matches!(
                    c,
                    RuntimeCall::Balances(pallet_balances::Call::transfer_allow_death { .. })
                )
            }
            ProxyType::JustUtility => matches!(c, RuntimeCall::Utility { .. }),
        }
    }
    fn is_superset(&self, o: &Self) -> bool {
        self == &ProxyType::Any || self == o
    }
}

/// Base call filter for the test runtime (blocks most `System` calls except `remark`).
pub struct BaseFilter;
impl Contains<RuntimeCall> for BaseFilter {
    fn contains(c: &RuntimeCall) -> bool {
        match *c {
            // Remark is used as a no-op call in the benchmarking
            RuntimeCall::System(SystemCall::remark { .. }) => true,
            RuntimeCall::System(_) => false,
            _ => true,
        }
    }
}

parameter_types! {
    pub static ProxyDepositBase: u64 = 1;
    pub static ProxyDepositFactor: u64 = 1;
    pub static AnnouncementDepositBase: u64 = 1;
    pub static AnnouncementDepositFactor: u64 = 1;
}

impl Config for Test {
    type RuntimeCall = RuntimeCall;
    type Currency = Balances;
    type ProxyType = ProxyType;
    type ProxyDepositBase = ProxyDepositBase;
    type ProxyDepositFactor = ProxyDepositFactor;
    type MaxProxies = ConstU32<4>;
    type WeightInfo = ();
    type CallHasher = BlakeTwo256;
    type MaxPending = ConstU32<2>;
    type AnnouncementDepositBase = AnnouncementDepositBase;
    type AnnouncementDepositFactor = AnnouncementDepositFactor;
    type BlockNumberProvider = frame_system::Pallet<Test>;
}

pub use crate::{Call as ProxyCall, Event as ProxyEvent};

pub type SystemError = frame_system::Error<Test>;

/// Build a test externalities with funded accounts `(1..=4)=10` and `5=3`.
pub fn new_test_ext() -> TestState {
    let mut t = frame_system::GenesisConfig::<Test>::default()
        .build_storage()
        .unwrap();
    pallet_balances::GenesisConfig::<Test> {
        balances: vec![(1, 10), (2, 10), (3, 10), (4, 10), (5, 3)],
        ..Default::default()
    }
    .assimilate_storage(&mut t)
    .unwrap();
    let mut ext = TestState::new(t);
    ext.execute_with(|| System::set_block_number(1));
    ext
}

pub fn last_events(n: usize) -> Vec<RuntimeEvent> {
    frame_system::Pallet::<Test>::events()
        .into_iter()
        .rev()
        .take(n)
        .rev()
        .map(|e| e.event)
        .collect()
}

pub fn expect_events(e: Vec<RuntimeEvent>) {
    assert_eq!(last_events(e.len()), e);
}

pub fn call_transfer(dest: u64, value: u64) -> RuntimeCall {
    RuntimeCall::Balances(BalancesCall::transfer_allow_death { dest, value })
}
