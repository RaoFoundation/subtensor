//! A minimal runtime for the pallet: `frame_system` plus lending over in-memory pools.
//!
//! The in-memory pools run the same constant-product curve as the pallet's quotes, so every
//! leg's result equals the quote the pallet gated on.

use core::cell::RefCell;
use std::collections::BTreeMap;

use frame_support::{
    PalletId, derive_impl,
    dispatch::DispatchResult,
    parameter_types,
    traits::{ConstU32, Hooks},
};
use sp_runtime::{BuildStorage, DispatchError};
use subtensor_runtime_common::NetUid;

use crate::{
    self as pallet_lending, Event, LendingPoolInterface,
    math::{
        curve::{ConstantProduct, Curve, PoolState},
        fixed::{Price, price},
    },
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
    /// Total alpha issued on the subnet (`alpha_issuance`).
    issuance: u64,
    moving: Price,
}

thread_local! {
    static POOLS: RefCell<BTreeMap<u16, PoolEntry>> = const { RefCell::new(BTreeMap::new()) };
    /// Free TAO balances by account.
    static BALANCES: RefCell<BTreeMap<u64, u64>> = const { RefCell::new(BTreeMap::new()) };
    /// Alpha held by an account on a subnet (custody keeps a long's locked alpha here).
    static HELD: RefCell<BTreeMap<(u64, u16), u64>> = const { RefCell::new(BTreeMap::new()) };
}

fn with_pool<R>(netuid: NetUid, f: impl FnOnce(&mut PoolEntry) -> R) -> R {
    POOLS.with_borrow_mut(|pools| f(pools.get_mut(&u16::from(netuid)).expect("pool exists")))
}

fn reserves(netuid: NetUid) -> PoolState {
    POOLS.with_borrow(|pools| {
        let p = pools.get(&u16::from(netuid)).expect("pool exists");
        PoolState {
            tao: p.tao,
            alpha: p.alpha,
        }
    })
}

fn set_reserves(netuid: NetUid, pool: PoolState) {
    with_pool(netuid, |p| {
        p.tao = pool.tao;
        p.alpha = pool.alpha;
    });
}

fn debit(who: u64, amount: u64) -> DispatchResult {
    BALANCES.with_borrow_mut(|b| {
        let bal = b.entry(who).or_insert(0);
        *bal = bal
            .checked_sub(amount)
            .ok_or(DispatchError::Other("mock: insufficient TAO"))?;
        Ok(())
    })
}

fn credit(who: u64, amount: u64) {
    BALANCES.with_borrow_mut(|b| {
        let bal = b.entry(who).or_insert(0);
        *bal = bal.saturating_add(amount);
    });
}

fn swap_error() -> DispatchError {
    DispatchError::Other("mock: swap not representable")
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
    fn subnet_alpha_in(netuid: NetUid) -> u64 {
        reserves(netuid).alpha
    }
    fn alpha_issuance(netuid: NetUid) -> u64 {
        POOLS.with_borrow(|pools| pools.get(&u16::from(netuid)).map_or(0, |p| p.issuance))
    }
    fn tao_balance(who: &u64) -> u64 {
        BALANCES.with_borrow(|b| b.get(who).copied().unwrap_or(0))
    }
    fn held_alpha(who: &u64, netuid: NetUid) -> u64 {
        HELD.with_borrow(|h| h.get(&(*who, u16::from(netuid))).copied().unwrap_or(0))
    }

    fn transfer_tao(from: &u64, to: &u64, amount: u64) -> DispatchResult {
        debit(*from, amount)?;
        credit(*to, amount);
        Ok(())
    }
    fn credit_pool_tao(netuid: NetUid, from: &u64, amount: u64) -> DispatchResult {
        debit(*from, amount)?;
        with_pool(netuid, |p| p.tao = p.tao.saturating_add(amount));
        Ok(())
    }
    fn mint_and_sell(netuid: NetUid, q: u64, to: &u64) -> Result<u64, DispatchError> {
        let (after, paid) =
            ConstantProduct::after_sell(&reserves(netuid), q).ok_or_else(swap_error)?;
        // The minted alpha is sold into the pool; the fee share stays issued but not in reserve.
        with_pool(netuid, |p| p.issuance = p.issuance.saturating_add(q));
        set_reserves(netuid, after);
        credit(*to, paid);
        Ok(paid)
    }
    fn buy_and_burn(netuid: NetUid, from: &u64, alpha: u64) -> Result<u64, DispatchError> {
        let (after, cost) =
            ConstantProduct::after_buy_alpha(&reserves(netuid), alpha).ok_or_else(swap_error)?;
        debit(*from, cost)?;
        set_reserves(netuid, after);
        with_pool(netuid, |p| p.issuance = p.issuance.saturating_sub(alpha));
        Ok(cost)
    }
    fn lift_and_buy(netuid: NetUid, lift: u64, to: &u64) -> Result<u64, DispatchError> {
        let pool = reserves(netuid);
        let lifted = PoolState {
            tao: pool.tao.checked_sub(lift).ok_or_else(swap_error)?,
            alpha: pool.alpha,
        };
        let (after, bought) =
            ConstantProduct::after_buy_tao(&lifted, lift).ok_or_else(swap_error)?;
        set_reserves(netuid, after);
        HELD.with_borrow_mut(|h| {
            let held = h.entry((*to, u16::from(netuid))).or_insert(0);
            *held = held.saturating_add(bought);
        });
        Ok(bought)
    }
    fn sell_alpha(netuid: NetUid, from: &u64, alpha: u64) -> Result<u64, DispatchError> {
        HELD.with_borrow_mut(|h| {
            let held = h.entry((*from, u16::from(netuid))).or_insert(0);
            *held = held
                .checked_sub(alpha)
                .ok_or(DispatchError::Other("mock: alpha not held"))?;
            Ok::<(), DispatchError>(())
        })?;
        let (after, paid) =
            ConstantProduct::after_sell(&reserves(netuid), alpha).ok_or_else(swap_error)?;
        set_reserves(netuid, after);
        credit(*from, paid);
        Ok(paid)
    }
}

/// Creates or replaces a live pool's reserves, keeping its moving price (zero for a new pool).
/// A new pool's issuance is its alpha reserve.
pub fn set_pool(netuid: u16, tao: u64, alpha: u64) {
    POOLS.with_borrow_mut(|pools| {
        let (moving, issuance) = pools
            .get(&netuid)
            .map_or((Price::from_bits(0), alpha), |p| (p.moving, p.issuance));
        pools.insert(
            netuid,
            PoolEntry {
                tao,
                alpha,
                issuance,
                moving,
            },
        );
    });
}

/// Sets a live pool's total alpha issuance.
pub fn set_issuance(netuid: u16, issuance: u64) {
    POOLS.with_borrow_mut(|pools| {
        pools.get_mut(&netuid).expect("pool exists").issuance = issuance;
    });
}

/// Sets an account's free TAO balance.
pub fn set_balance(who: u64, amount: u64) {
    BALANCES.with_borrow_mut(|b| {
        b.insert(who, amount);
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

/// Fresh externalities at block 1 with no pools, no balances and storage version 0 (nothing
/// migrated).
pub fn new_test_ext() -> sp_io::TestExternalities {
    POOLS.with_borrow_mut(BTreeMap::clear);
    BALANCES.with_borrow_mut(BTreeMap::clear);
    HELD.with_borrow_mut(BTreeMap::clear);
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
