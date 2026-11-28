//! A minimal runtime for the pallet: `frame_system` plus lending over in-memory pools.
//!
//! `Curve` is the model's `ConstantProduct`, and the in-memory pools (50/50, the model fee) run
//! the same curve, so every leg's result equals the quote the pallet gated on.

use core::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use frame_support::{
    PalletId, derive_impl,
    dispatch::DispatchResult,
    parameter_types,
    traits::{ConstU32, Hooks},
};
use sp_arithmetic::Perquintill;
use sp_runtime::{BuildStorage, DispatchError, traits::IdentityLookup};
use subtensor_runtime_common::NetUid;

use crate::{
    self as pallet_lending, Event, LendingPoolInterface,
    math::{
        curve::{ConstantProduct, Curve, FeeRate, HALF_WEIGHT, MODEL_FEE, PoolState},
        fixed::{Price, price},
    },
    params::MAX_LONG_POSITIONS_CEILING,
    weights::SubstrateWeight,
};

type Block = frame_system::mocking::MockBlock<Test>;

/// Wide enough that a custody account keeps bytes of its hashed seed after the pallet id, so
/// every position has its own custody as on chain.
pub type AccountId = u128;

frame_support::construct_runtime!(
    pub enum Test {
        System: frame_system = 1,
        Lending: pallet_lending = 2,
    }
);

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl frame_system::Config for Test {
    type Block = Block;
    type AccountId = AccountId;
    type Lookup = IdentityLookup<AccountId>;
}

parameter_types! {
    pub const LendingPalletId: PalletId = PalletId(*b"bt/lendg");
}

impl pallet_lending::Config for Test {
    type Pool = FakePool;
    type Curve = ConstantProduct;
    type WeightInfo = SubstrateWeight<Test>;
    type PalletId = LendingPalletId;
    type MaxLongPositionsCeiling = ConstU32<MAX_LONG_POSITIONS_CEILING>;
    type MaxChargesPerBlock = ConstU32<512>;
    #[cfg(feature = "runtime-benchmarks")]
    type BenchmarkHelper = FakePoolBenchmarkHelper;
}

/// Builds the benchmarks' pools in [`FakePool`]: the same storage of this pallet is read and
/// written as on chain, and the pool legs cost nothing.
#[cfg(feature = "runtime-benchmarks")]
pub struct FakePoolBenchmarkHelper;

#[cfg(feature = "runtime-benchmarks")]
impl crate::BenchmarkHelper<AccountId> for FakePoolBenchmarkHelper {
    fn clear_subnets() {
        POOLS.with_borrow_mut(BTreeMap::clear);
    }
    fn setup_pool(netuid: NetUid, tao: u64, alpha: u64) {
        let netuid = u16::from(netuid);
        remove_pool(netuid);
        set_pool(netuid, tao, alpha);
        set_moving_price(netuid, px(tao, alpha));
    }
    fn fund(who: &AccountId, tao: u64) {
        credit(*who, tao);
    }
    fn set_block_author() {}
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
    static BALANCES: RefCell<BTreeMap<AccountId, u64>> = const { RefCell::new(BTreeMap::new()) };
    /// Alpha held by an account on a subnet (custody keeps a long's locked alpha here).
    static HELD: RefCell<BTreeMap<(AccountId, u16), u64>> = const { RefCell::new(BTreeMap::new()) };
    /// Subnets taken out of the emission set; every other live pool is in it.
    static OUT_OF_EMISSION: RefCell<BTreeSet<u16>> = const { RefCell::new(BTreeSet::new()) };
    /// TAO burned through `burn_tao`.
    static BURNED: RefCell<u64> = const { RefCell::new(0) };
    /// The `(coldkey, hotkey)` the pallet hotkey was registered with.
    static HOTKEY_OWNER: RefCell<Option<(AccountId, AccountId)>> = const { RefCell::new(None) };
}

/// The `(coldkey, hotkey)` registered through `register_pallet_hotkey`.
pub fn registered_pallet_hotkey() -> Option<(AccountId, AccountId)> {
    HOTKEY_OWNER.with_borrow(|owner| *owner)
}

fn with_pool<R>(netuid: NetUid, f: impl FnOnce(&mut PoolEntry) -> R) -> R {
    POOLS.with_borrow_mut(|pools| f(pools.get_mut(&u16::from(netuid)).expect("pool exists")))
}

fn reserves(netuid: NetUid) -> PoolState {
    POOLS.with_borrow(|pools| {
        let p = pools.get(&u16::from(netuid)).expect("pool exists");
        PoolState::model(p.tao, p.alpha)
    })
}

fn set_reserves(netuid: NetUid, pool: PoolState) {
    with_pool(netuid, |p| {
        p.tao = pool.tao;
        p.alpha = pool.alpha;
    });
}

fn debit(who: AccountId, amount: u64) -> DispatchResult {
    BALANCES.with_borrow_mut(|b| {
        let bal = b.entry(who).or_insert(0);
        *bal = bal
            .checked_sub(amount)
            .ok_or(DispatchError::Other("mock: insufficient TAO"))?;
        Ok(())
    })
}

fn credit(who: AccountId, amount: u64) {
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

impl LendingPoolInterface<AccountId> for FakePool {
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
    fn quote_weight(_netuid: NetUid) -> Perquintill {
        HALF_WEIGHT
    }
    fn swap_fee(_netuid: NetUid) -> FeeRate {
        MODEL_FEE
    }
    fn alpha_issuance(netuid: NetUid) -> u64 {
        POOLS.with_borrow(|pools| pools.get(&u16::from(netuid)).map_or(0, |p| p.issuance))
    }
    fn is_in_emission_set(netuid: NetUid) -> bool {
        let netuid = u16::from(netuid);
        POOLS.with_borrow(|pools| pools.contains_key(&netuid))
            && OUT_OF_EMISSION.with_borrow(|out| !out.contains(&netuid))
    }
    fn tao_balance(who: &AccountId) -> u64 {
        BALANCES.with_borrow(|b| b.get(who).copied().unwrap_or(0))
    }
    fn held_alpha(who: &AccountId, netuid: NetUid) -> u64 {
        HELD.with_borrow(|h| h.get(&(*who, u16::from(netuid))).copied().unwrap_or(0))
    }

    fn register_pallet_hotkey(coldkey: &AccountId, hotkey: &AccountId) -> DispatchResult {
        HOTKEY_OWNER.with_borrow_mut(|owner| match *owner {
            Some((cold, hot)) if cold == *coldkey && hot == *hotkey => Ok(()),
            Some(_) => Err(DispatchError::Other("mock: pallet hotkey registered twice")),
            None => {
                *owner = Some((*coldkey, *hotkey));
                Ok(())
            }
        })
    }
    fn transfer_tao(from: &AccountId, to: &AccountId, amount: u64) -> Result<u64, DispatchError> {
        debit(*from, amount)?;
        credit(*to, amount);
        Ok(amount)
    }
    fn credit_pool_tao(
        netuid: NetUid,
        from: &AccountId,
        amount: u64,
    ) -> Result<u64, DispatchError> {
        debit(*from, amount)?;
        with_pool(netuid, |p| p.tao = p.tao.saturating_add(amount));
        Ok(amount)
    }
    fn burn_tao(from: &AccountId, amount: u64) -> Result<u64, DispatchError> {
        debit(*from, amount)?;
        BURNED.with_borrow_mut(|burned| *burned = burned.saturating_add(amount));
        Ok(amount)
    }
    fn mint_and_sell(netuid: NetUid, q: u64, to: &AccountId) -> Result<u64, DispatchError> {
        let (after, paid) =
            ConstantProduct::after_sell(&reserves(netuid), q).ok_or_else(swap_error)?;
        // The minted alpha is sold into the pool; the fee share stays issued but not in reserve.
        with_pool(netuid, |p| p.issuance = p.issuance.saturating_add(q));
        set_reserves(netuid, after);
        credit(*to, paid);
        Ok(paid)
    }
    fn buy_and_burn(netuid: NetUid, from: &AccountId, alpha: u64) -> Result<u64, DispatchError> {
        let (after, cost) =
            ConstantProduct::after_buy_alpha(&reserves(netuid), alpha).ok_or_else(swap_error)?;
        debit(*from, cost)?;
        set_reserves(netuid, after);
        with_pool(netuid, |p| p.issuance = p.issuance.saturating_sub(alpha));
        Ok(cost)
    }
    fn lift_and_buy(netuid: NetUid, lift: u64, to: &AccountId) -> Result<u64, DispatchError> {
        let pool = reserves(netuid);
        let lifted = pool.with_reserves(
            pool.tao.checked_sub(lift).ok_or_else(swap_error)?,
            pool.alpha,
        );
        let (after, bought) =
            ConstantProduct::after_buy_tao(&lifted, lift).ok_or_else(swap_error)?;
        set_reserves(netuid, after);
        HELD.with_borrow_mut(|h| {
            let held = h.entry((*to, u16::from(netuid))).or_insert(0);
            *held = held.saturating_add(bought);
        });
        Ok(bought)
    }
    fn sell_alpha(netuid: NetUid, from: &AccountId, alpha: u64) -> Result<u64, DispatchError> {
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
pub fn set_balance(who: AccountId, amount: u64) {
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

/// Puts a subnet in the emission set or takes it out of it.
pub fn set_in_emission(netuid: u16, in_set: bool) {
    OUT_OF_EMISSION.with_borrow_mut(|out| {
        if in_set {
            out.remove(&netuid);
        } else {
            out.insert(netuid);
        }
    });
}

/// TAO burned through the pool's `burn_tao` since the externalities were built.
pub fn burned_tao() -> u64 {
    BURNED.with_borrow(|burned| *burned)
}

/// The sum of every free TAO balance: holders and custody accounts.
pub fn total_balances() -> u128 {
    BALANCES.with_borrow(|b| b.values().map(|v| u128::from(*v)).sum())
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
    OUT_OF_EMISSION.with_borrow_mut(BTreeSet::clear);
    BURNED.with_borrow_mut(|burned| *burned = 0);
    HOTKEY_OWNER.with_borrow_mut(|owner| *owner = None);
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

/// Writes a change to the live `LendingParams`, as an applied setter leaves it.
pub fn set_live_lending(change: impl FnOnce(&mut crate::params::LendingParams)) {
    crate::LendingParamsStore::<Test>::mutate(change);
}

/// Seeds both slow prices of a subnet at `price`: the TWAP ring and the SMA30 rank ring, each a
/// full window of `price`. A short's debt reads the lower of the two and a long's the higher
/// (SPEC 2.1), so this moves a position's debt over its holdings without moving the pool.
pub fn seed_slow_prices(netuid: u16, price: Price) {
    let netuid = NetUid::from(netuid);
    let samples = crate::LendingParamsStore::<Test>::get().twap_samples;
    crate::TwapRing::<Test>::insert(netuid, crate::prices::TwapState::seeded(price, samples));
    crate::RankRing::<Test>::insert(netuid, crate::prices::RankState::seeded(price, 30));
    crate::RankPrice::<Test>::insert(netuid, price);
}

/// Sets the curve of the tests of the rate (exponent 2, cap 100%: `LAUNCH` in `tests/interest.rs`)
/// and no delay of a rate increase, so that a tightening applies in the next block.
pub fn bare_curve() {
    set_live_lending(|p| {
        p.rate_k = 2;
        p.rate_cap_bps = 10_000;
        p.rate_delay_blocks = 0;
    });
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
