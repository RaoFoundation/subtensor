//! A minimal runtime for the pallet: `frame_system` plus lending over in-memory pools.

use core::cell::RefCell;
use std::collections::BTreeMap;

use frame_support::{
    PalletId, derive_impl, parameter_types,
    traits::{ConstU32, Hooks},
};
use sp_runtime::BuildStorage;
use subtensor_runtime_common::NetUid;

use crate::{
    self as pallet_lending, Event, LendingPoolInterface,
    math::fixed::{Price, price},
    params::MAX_LONG_POSITIONS_CEILING,
    weights::SubstrateWeight,
};

type Block = frame_system::mocking::MockBlock<Test>;

frame_support::construct_runtime!(
    pub enum Test {
        System: frame_system = 1,
        Lending: pallet_lending = 2,
    }
);

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl frame_system::Config for Test {
    type Block = Block;
}

parameter_types! {
    pub const LendingPalletId: PalletId = PalletId(*b"bt/lendg");
}

impl pallet_lending::Config for Test {
    type Pool = FakePool;
    type WeightInfo = SubstrateWeight<Test>;
    type PalletId = LendingPalletId;
    type MaxLongPositionsCeiling = ConstU32<MAX_LONG_POSITIONS_CEILING>;
}

/// Subnet limit reported by [`FakePool`].
pub const SUBNET_LIMIT: u32 = 128;

/// One in-memory subnet pool.
#[derive(Clone, Copy)]
struct PoolEntry {
    tao: u64,
    alpha: u64,
    moving: Price,
}

thread_local! {
    static POOLS: RefCell<BTreeMap<u16, PoolEntry>> = const { RefCell::new(BTreeMap::new()) };
}

/// `LendingPoolInterface` over the thread-local pools; every pool in the map is live.
pub struct FakePool;

impl LendingPoolInterface<u64> for FakePool {
    fn live_subnets() -> Vec<NetUid> {
        POOLS.with_borrow(|pools| pools.keys().copied().map(NetUid::from).collect())
    }
    fn subnet_limit() -> u32 {
        SUBNET_LIMIT
    }
    fn spot_price(netuid: NetUid) -> Price {
        POOLS.with_borrow(|pools| {
            pools
                .get(&u16::from(netuid))
                .map_or(Price::from_bits(0), |p| price(p.tao, u128::from(p.alpha)))
        })
    }
    fn pool_tao(netuid: NetUid) -> u64 {
        POOLS.with_borrow(|pools| pools.get(&u16::from(netuid)).map_or(0, |p| p.tao))
    }
    fn moving_alpha_price(netuid: NetUid) -> Price {
        POOLS.with_borrow(|pools| {
            pools
                .get(&u16::from(netuid))
                .map_or(Price::from_bits(0), |p| p.moving)
        })
    }
}

/// Creates or replaces a live pool's reserves, keeping its moving price (zero for a new pool).
pub fn set_pool(netuid: u16, tao: u64, alpha: u64) {
    POOLS.with_borrow_mut(|pools| {
        let moving = pools.get(&netuid).map_or(Price::from_bits(0), |p| p.moving);
        pools.insert(netuid, PoolEntry { tao, alpha, moving });
    });
}

/// Sets a live pool's emission moving price (`SubnetMovingPrice`).
pub fn set_moving_price(netuid: u16, moving: Price) {
    POOLS.with_borrow_mut(|pools| {
        pools.get_mut(&netuid).expect("pool exists").moving = moving;
    });
}

/// Removes a pool: the subnet is no longer live.
pub fn remove_pool(netuid: u16) {
    POOLS.with_borrow_mut(|pools| {
        pools.remove(&netuid);
    });
}

/// The price of a pool with these reserves.
pub fn px(tao: u64, alpha: u64) -> Price {
    price(tao, u128::from(alpha))
}

/// Fresh externalities at block 1 with no pools and storage version 0 (nothing migrated).
pub fn new_test_ext() -> sp_io::TestExternalities {
    POOLS.with_borrow_mut(BTreeMap::clear);
    let storage = frame_system::GenesisConfig::<Test>::default()
        .build_storage()
        .expect("system genesis builds");
    let mut ext = sp_io::TestExternalities::new(storage);
    ext.execute_with(|| System::set_block_number(1));
    ext
}

/// Externalities with two live subnets (1 and 2) after the v1 migration.
///
/// SN1: 1,000 TAO / 10,000 alpha (spot 0.1), moving price 0.08.
/// SN2: 5,000 TAO / 10,000 alpha (spot 0.5), moving price 0.4.
pub fn migrated_ext() -> sp_io::TestExternalities {
    let mut ext = new_test_ext();
    ext.execute_with(|| {
        set_pool(1, 1_000, 10_000);
        set_moving_price(1, px(800, 10_000));
        set_pool(2, 5_000, 10_000);
        set_moving_price(2, px(4_000, 10_000));
        Lending::on_runtime_upgrade();
    });
    ext
}

/// Runs `on_initialize` for every block after the current one up to and including `target`.
/// Events of earlier blocks are dropped, so `lending_events` returns only `target`'s.
pub fn run_to(target: u64) {
    while System::block_number() < target {
        System::reset_events();
        let now = System::block_number() + 1;
        System::set_block_number(now);
        Lending::on_initialize(now);
    }
}

/// Runs one block.
pub fn next_block() {
    run_to(System::block_number() + 1);
}

/// Lending events deposited since the current block began.
pub fn lending_events() -> Vec<Event<Test>> {
    System::events()
        .into_iter()
        .filter_map(|record| match record.event {
            RuntimeEvent::Lending(event) => Some(event),
            _ => None,
        })
        .collect()
}
