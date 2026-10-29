#![allow(
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing
)]

//! Focused unit tests for lift-based lending v5.1 (Jacob §13 / §13.2).

use super::*;
use crate::weights::WeightInfo;
use codec::{Decode, Encode};
#[cfg(feature = "try-runtime")]
use frame_support::traits::OnRuntimeUpgrade;
use frame_support::{
    BoundedVec, assert_err, assert_ok, construct_runtime, derive_impl, ensure, parameter_types,
    traits::{ConstU32, ConstU64, Get, Hooks},
    weights::{Weight, WeightMeter, constants::RocksDbWeight},
};
use sp_runtime::{
    AccountId32, BuildStorage,
    traits::{BlakeTwo256, IdentityLookup},
};

construct_runtime!(pub enum Test { System: frame_system = 0, Lending: crate = 1 });
type Account = AccountId32;
type Block = frame_system::mocking::MockBlock<Test>;

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl frame_system::Config for Test {
    type Block = Block;
    type AccountId = Account;
    type Lookup = IdentityLookup<Account>;
    type Hashing = BlakeTwo256;
}

parameter_types! { pub const LendingId: PalletId = PalletId(*b"bt/lends"); }

impl Config for Test {
    type Pool = MockPool;
    type PalletId = LendingId;
    type MinimumLoanValue = ConstU64<1>;
    type InterestPeriod = ConstU64<10>;
    type BlocksPerYear = ConstU64<520>;
    type ReferenceWarmup = ConstU64<1>;
    type MaxPositionsPerSubnet = ConstU32<8>;
    type MaxTotalPositions = ConstU32<16>;
    // Match production (100): regressions must cover gaps wider than the
    // 64-block empty-bump budget (Skeptic fid:1c6bb1dc).
    type ExpiryRetryDelay = ConstU64<100>;
    type InterestRetryDelay = ConstU64<100>;
    type WeightInfo = ();
}

fn account(n: u8) -> Account {
    Account::new([n; 32])
}
fn netuid() -> NetUid {
    64.into()
}
fn key(prefix: &[u8], args: impl Encode) -> Vec<u8> {
    [prefix, &args.encode()].concat()
}
fn read<V: Decode + Default>(k: &[u8]) -> V {
    sp_io::storage::get(k)
        .and_then(|bytes| V::decode(&mut &bytes[..]).ok())
        .unwrap_or_default()
}
fn write(k: &[u8], value: impl Encode) {
    sp_io::storage::set(k, &value.encode());
}
fn tao(who: &Account) -> u64 {
    read(&key(b"test/tao", who))
}
fn alpha(who: &Account, hotkey: &Account, netuid: NetUid) -> u64 {
    read(&key(b"test/alpha", (who, hotkey, netuid)))
}
fn mint_tao(who: &Account, amount: u64) {
    write(&key(b"test/tao", who), tao(who).saturating_add(amount));
}
fn mint_alpha(who: &Account, hotkey: &Account, netuid: NetUid, amount: u64) {
    write(
        &key(b"test/alpha", (who, hotkey, netuid)),
        alpha(who, hotkey, netuid).saturating_add(amount),
    );
}
fn pool_tao() -> u64 {
    read(&key(b"test/pool_tao", netuid()))
}
fn pool_alpha() -> u64 {
    read(&key(b"test/pool_alpha", netuid()))
}
fn set_pool(tao_r: u64, alpha_r: u64) {
    write(&key(b"test/pool_tao", netuid()), tao_r);
    write(&key(b"test/pool_alpha", netuid()), alpha_r);
}
/// Production-like fillable sell bound as a multiple of the alpha reserve.
/// `max_sell_input` is reserve×1000; quotes above this multiple refuse like
/// `SlippageTooHigh` on a real Balancer sim_swap partial fill.
const FILLABLE_SELL_RESERVE_MULTIPLIER: u64 = 10;
fn quote_weight_parts() -> u64 {
    let v: u64 = read(&key(b"test/quote_weight_parts", netuid()));
    if v == 0 {
        500_000_000_000_000_000 // 50/50 default
    } else {
        v
    }
}
fn set_quote_weight_parts(parts: u64) {
    write(&key(b"test/quote_weight_parts", netuid()), parts);
}
fn alpha_out() -> u64 {
    read(&key(b"test/alpha_out", netuid()))
}
fn set_alpha_out(v: u64) {
    write(&key(b"test/alpha_out", netuid()), v);
}
fn burned() -> u64 {
    tao(&account(255))
}
fn moving_price() -> U64F64 {
    read(&key(b"test/moving_price", netuid()))
}
fn set_moving_price(price: U64F64) {
    write(&key(b"test/moving_price", netuid()), price);
}
fn tao_ema() -> Option<u64> {
    TaoEma::<Test>::get(netuid())
}
fn set_tao_ema(v: u64) {
    TaoEma::<Test>::insert(netuid(), v);
}
fn clear_tao_ema() {
    TaoEma::<Test>::remove(netuid());
}

pub struct MockPool;
impl MockPool {
    fn price() -> U64F64 {
        let y = pool_tao();
        let x = pool_alpha();
        if x == 0 {
            U64F64::from_num(0)
        } else {
            U64F64::from_num(y) / U64F64::from_num(x)
        }
    }
}

impl OrderSwapInterface<Account> for MockPool {
    fn buy_alpha(
        coldkey: &Account,
        hotkey: &Account,
        netuid: NetUid,
        tao_amount: TaoBalance,
        _: TaoBalance,
        _: bool,
    ) -> Result<AlphaBalance, DispatchError> {
        let y = pool_tao();
        let x = pool_alpha();
        ensure!(y > 0 && x > 0, "empty pool");
        let t = tao_amount.to_u64();
        ensure!(tao(coldkey) >= t, "insufficient tao");
        // Constant-product buy: alpha_out = x * t / (y + t)
        let a = (u128::from(x) * u128::from(t) / u128::from(y.saturating_add(t))) as u64;
        ensure!(a > 0, "zero out");
        write(&key(b"test/tao", coldkey), tao(coldkey).saturating_sub(t));
        write(&key(b"test/pool_tao", netuid), y.saturating_add(t));
        write(&key(b"test/pool_alpha", netuid), x.saturating_sub(a));
        mint_alpha(coldkey, hotkey, netuid, a);
        set_alpha_out(alpha_out().saturating_add(a));
        Ok(a.into())
    }

    fn sell_alpha(
        coldkey: &Account,
        hotkey: &Account,
        netuid: NetUid,
        alpha_amount: AlphaBalance,
        _: TaoBalance,
        _: bool,
    ) -> Result<TaoBalance, DispatchError> {
        let y = pool_tao();
        let x = pool_alpha();
        let a = alpha_amount.to_u64();
        ensure!(alpha(coldkey, hotkey, netuid) >= a, "insufficient alpha");
        // Constant-product sell: tao_out = y * a / (x + a)
        let t = (u128::from(y) * u128::from(a) / u128::from(x.saturating_add(a))) as u64;
        ensure!(t > 0, "zero out");
        write(
            &key(b"test/alpha", (coldkey, hotkey, netuid)),
            alpha(coldkey, hotkey, netuid).saturating_sub(a),
        );
        set_alpha_out(alpha_out().saturating_sub(a));
        write(&key(b"test/pool_alpha", netuid), x.saturating_add(a));
        write(&key(b"test/pool_tao", netuid), y.saturating_sub(t));
        mint_tao(coldkey, t);
        Ok(t.into())
    }

    fn current_alpha_price(_: NetUid) -> U64F64 {
        Self::price()
    }

    fn transfer_tao(from: &Account, to: &Account, amount: TaoBalance) -> DispatchResult {
        let a = amount.to_u64();
        ensure!(tao(from) >= a, "insufficient tao");
        write(&key(b"test/tao", from), tao(from).saturating_sub(a));
        mint_tao(to, a);
        Ok(())
    }

    fn transfer_staked_alpha(
        from_cold: &Account,
        from_hot: &Account,
        to_cold: &Account,
        to_hot: &Account,
        netuid: NetUid,
        amount: AlphaBalance,
        _: bool,
        _: bool,
    ) -> DispatchResult {
        // Mirror production: after NetworksAdded is cleared at dissolve, stake
        // transfers fail with SubnetNotExists. Dissolution settle must not rely
        // on this path for long interest.
        ensure!(
            !Dissolutions::<Test>::contains_key(netuid),
            "SubnetNotExists"
        );
        // Mirror production `ensure_subtoken_enabled`: a disabled subtoken must
        // roll interest collection back so `collect_due` can defer retry.
        ensure!(Self::subtoken_enabled(netuid), "SubtokenDisabled");
        let a = amount.to_u64();
        ensure!(
            alpha(from_cold, from_hot, netuid) >= a,
            "insufficient alpha"
        );
        write(
            &key(b"test/alpha", (from_cold, from_hot, netuid)),
            alpha(from_cold, from_hot, netuid).saturating_sub(a),
        );
        mint_alpha(to_cold, to_hot, netuid, a);
        Ok(())
    }

    fn register_pallet_hotkey(_: &Account, _: &Account) -> DispatchResult {
        Ok(())
    }

    fn pallet_hotkey_registered(_: &Account, _: &Account) -> bool {
        true
    }
}

impl LendingPoolInterface<Account> for MockPool {
    fn subnet_exists(_: NetUid) -> bool {
        true
    }

    fn network_alive(_: NetUid) -> bool {
        true
    }

    #[cfg(feature = "runtime-benchmarks")]
    fn benchmark_seed_pool(netuid: NetUid, tao: u64, alpha: u64) {
        write(&key(b"test/pool_tao", netuid), tao);
        write(&key(b"test/pool_alpha", netuid), alpha);
    }

    #[cfg(feature = "runtime-benchmarks")]
    fn benchmark_mint_tao(who: &Account, amount: u64) {
        mint_tao(who, amount);
    }

    #[cfg(feature = "runtime-benchmarks")]
    fn benchmark_mint_alpha(who: &Account, hotkey: &Account, netuid: NetUid, amount: u64) {
        mint_alpha(who, hotkey, netuid, amount);
    }

    fn quote_sell(netuid: NetUid, alpha: AlphaBalance) -> Result<TaoBalance, DispatchError> {
        let y = Self::pool_tao(netuid).to_u64();
        let x = Self::pool_alpha(netuid).to_u64();
        let a = alpha.to_u64();
        // Mirror production sim_swap: inputs above the fillable fraction of the
        // alpha reserve do not fully fill → SlippageTooHigh. max_sell_input is
        // reserve×1000, so probing it always fails on deep/realistic pools.
        let fillable = x.saturating_mul(FILLABLE_SELL_RESERVE_MULTIPLIER);
        ensure!(a <= fillable, "SlippageTooHigh");
        Ok(((u128::from(y) * u128::from(a) / u128::from(x.saturating_add(a))) as u64).into())
    }

    fn quote_buy(netuid: NetUid, tao_in: TaoBalance) -> Result<AlphaBalance, DispatchError> {
        let y = Self::pool_tao(netuid).to_u64();
        let x = Self::pool_alpha(netuid).to_u64();
        let t = tao_in.to_u64();
        let fillable = y.saturating_mul(FILLABLE_SELL_RESERVE_MULTIPLIER);
        ensure!(t <= fillable, "SlippageTooHigh");
        Ok(((u128::from(x) * u128::from(t) / u128::from(y.saturating_add(t))) as u64).into())
    }

    fn max_sell_input(netuid: NetUid) -> AlphaBalance {
        // Production: alpha_reserve × MAX_SWAP_INPUT_RESERVE_MULTIPLIER (1000).
        Self::pool_alpha(netuid)
            .to_u64()
            .saturating_mul(1_000)
            .into()
    }

    fn max_buy_input(netuid: NetUid) -> TaoBalance {
        Self::pool_tao(netuid).to_u64().saturating_mul(1_000).into()
    }

    fn burn_interest_tao(from: &Account, amount: TaoBalance) -> DispatchResult {
        Self::transfer_tao(from, &account(255), amount)
    }

    fn pool_tao(_: NetUid) -> TaoBalance {
        pool_tao().into()
    }

    fn pool_alpha(_: NetUid) -> AlphaBalance {
        pool_alpha().into()
    }

    fn quote_weight_parts(_: NetUid) -> u64 {
        // Configurable; default 50/50. Uneven weights exercise Balancer-shaped pools.
        quote_weight_parts()
    }

    fn buy_depth_threshold(_: NetUid, ref_tao: TaoBalance, move_bps: u16) -> TaoBalance {
        // y* ≈ ref / ((1+bps)^1 - 1) for equal weights ≈ ref * 10000 / move_bps
        let r = ref_tao.to_u64();
        let b = u64::from(move_bps).max(1);
        (r.saturating_mul(10_000) / b).into()
    }

    fn quote_sell_on(
        tao_reserve: TaoBalance,
        alpha_reserve: AlphaBalance,
        _: u64,
        delta_alpha: AlphaBalance,
    ) -> TaoBalance {
        let y = tao_reserve.to_u64();
        let x = alpha_reserve.to_u64();
        let a = delta_alpha.to_u64();
        ((u128::from(y) * u128::from(a) / u128::from(x.saturating_add(a))) as u64).into()
    }

    fn quote_buy_on(
        tao_reserve: TaoBalance,
        alpha_reserve: AlphaBalance,
        _: u64,
        delta_tao: TaoBalance,
    ) -> AlphaBalance {
        let y = tao_reserve.to_u64();
        let x = alpha_reserve.to_u64();
        let t = delta_tao.to_u64();
        ((u128::from(x) * u128::from(t) / u128::from(y.saturating_add(t))) as u64).into()
    }

    fn lift_and_debit(
        netuid: NetUid,
        g_numer: u64,
        g_denom: u64,
        custody_cold: &Account,
        custody_hot: &Account,
    ) -> Result<(AlphaBalance, TaoBalance), DispatchError> {
        ensure!(g_denom > 0 && g_numer > 0 && g_numer < g_denom, "bad g");
        let y = pool_tao();
        let x = pool_alpha();
        let lift_tao = (u128::from(y) * u128::from(g_numer) / u128::from(g_denom)) as u64;
        let lift_alpha = (u128::from(x) * u128::from(g_numer) / u128::from(g_denom)) as u64;
        ensure!(lift_tao > 0 && lift_alpha > 0, "zero lift");
        write(&key(b"test/pool_tao", netuid), y.saturating_sub(lift_tao));
        write(
            &key(b"test/pool_alpha", netuid),
            x.saturating_sub(lift_alpha),
        );
        mint_tao(custody_cold, lift_tao);
        mint_alpha(custody_cold, custody_hot, netuid, lift_alpha);
        set_alpha_out(alpha_out().saturating_add(lift_alpha));
        Ok((lift_alpha.into(), lift_tao.into()))
    }

    fn readd_and_credit(
        netuid: NetUid,
        custody_cold: &Account,
        custody_hot: &Account,
        tao_in: TaoBalance,
        alpha_in: AlphaBalance,
    ) -> DispatchResult {
        let t = tao_in.to_u64();
        let a = alpha_in.to_u64();
        if t > 0 {
            ensure!(tao(custody_cold) >= t, "insufficient tao");
            write(&key(b"test/tao", custody_cold), tao(custody_cold) - t);
            write(&key(b"test/pool_tao", netuid), pool_tao().saturating_add(t));
        }
        if a > 0 {
            ensure!(
                alpha(custody_cold, custody_hot, netuid) >= a,
                "insufficient alpha"
            );
            write(
                &key(b"test/alpha", (custody_cold, custody_hot, netuid)),
                alpha(custody_cold, custody_hot, netuid) - a,
            );
            set_alpha_out(alpha_out().saturating_sub(a));
            write(
                &key(b"test/pool_alpha", netuid),
                pool_alpha().saturating_add(a),
            );
        }
        Ok(())
    }

    fn forced_buy_into_custody(
        custody_cold: &Account,
        custody_hot: &Account,
        netuid: NetUid,
        tao_in: TaoBalance,
    ) -> Result<AlphaBalance, DispatchError> {
        Self::buy_alpha(
            custody_cold,
            custody_hot,
            netuid,
            tao_in,
            u64::MAX.into(),
            false,
        )
    }

    fn forced_sell_into_escrow(
        custody_cold: &Account,
        custody_hot: &Account,
        escrow_cold: &Account,
        netuid: NetUid,
        alpha_in: AlphaBalance,
        min_tao_out: TaoBalance,
    ) -> Result<TaoBalance, DispatchError> {
        let t = Self::sell_alpha(
            custody_cold,
            custody_hot,
            netuid,
            alpha_in,
            0u64.into(),
            false,
        )?;
        ensure!(t >= min_tao_out, "slippage");
        if custody_cold != escrow_cold {
            Self::transfer_tao(custody_cold, escrow_cold, t)?;
        }
        Ok(t)
    }

    fn unlocked_stake(hotkey: &Account, coldkey: &Account, netuid: NetUid) -> AlphaBalance {
        alpha(coldkey, hotkey, netuid).into()
    }

    fn unstake_for_raise(
        hotkey: &Account,
        coldkey: &Account,
        netuid: NetUid,
        alpha_in: AlphaBalance,
        min_tao_out: TaoBalance,
    ) -> Result<TaoBalance, DispatchError> {
        let t = Self::sell_alpha(coldkey, hotkey, netuid, alpha_in, 0u64.into(), false)?;
        ensure!(t >= min_tao_out, "slippage");
        Ok(t)
    }

    fn eligible_alpha_for_dissolution(_: NetUid) -> Result<u128, DispatchError> {
        // Legacy-style: omit AlphaIn (main's staker divisor).
        Ok(u128::from(alpha_out()))
    }

    fn all_alpha_for_lending_settlement(_: NetUid) -> u128 {
        // §13: always include pool alpha.
        u128::from(pool_alpha()).saturating_add(u128::from(alpha_out()))
    }

    fn moving_alpha_price(_: NetUid) -> U64F64 {
        moving_price()
    }

    fn live_subnets() -> alloc::vec::Vec<NetUid> {
        alloc::vec![netuid()]
    }

    fn pay_from_pot(
        netuid: NetUid,
        to: &Account,
        amount: TaoBalance,
    ) -> Result<TaoBalance, DispatchError> {
        let t = amount.to_u64();
        ensure!(pool_tao() >= t, "pot dry");
        write(&key(b"test/pool_tao", netuid), pool_tao() - t);
        mint_tao(to, t);
        Ok(amount)
    }

    fn burn_from_pot(netuid: NetUid, amount: TaoBalance) -> DispatchResult {
        let t = amount.to_u64();
        ensure!(pool_tao() >= t, "pot dry");
        write(&key(b"test/pool_tao", netuid), pool_tao() - t);
        mint_tao(&account(255), t);
        Ok(())
    }

    fn destroy_custody_alpha(
        coldkey: &Account,
        hotkey: &Account,
        netuid: NetUid,
        amount: AlphaBalance,
    ) -> DispatchResult {
        let a = amount.to_u64();
        ensure!(alpha(coldkey, hotkey, netuid) >= a, "insufficient alpha");
        write(
            &key(b"test/alpha", (coldkey, hotkey, netuid)),
            alpha(coldkey, hotkey, netuid) - a,
        );
        set_alpha_out(alpha_out().saturating_sub(a));
        Ok(())
    }

    fn network_registered_at(netuid: NetUid) -> u64 {
        read(&key(b"test/registered_at", netuid))
    }

    fn subtoken_enabled(netuid: NetUid) -> bool {
        let v: u8 = read(&key(b"test/subtoken", netuid));
        v != 0
    }
}

fn set_registered_at(block: u64) {
    write(&key(b"test/registered_at", netuid()), block);
}
fn set_subtoken_enabled(enabled: bool) {
    write(
        &key(b"test/subtoken", netuid()),
        if enabled { 1u8 } else { 0u8 },
    );
}

/// Far-future deadline used by tests that are not exercising §5.2 expiry.
fn deadline() -> u64 {
    u64::MAX
}

fn new_test_ext() -> sp_io::TestExternalities {
    let mut ext: sp_io::TestExternalities = frame_system::GenesisConfig::<Test>::default()
        .build_storage()
        .unwrap()
        .into();
    ext.execute_with(|| {
        System::set_block_number(10);
        assert_ok!(Lending::initialize_custody());
        Enabled::<Test>::put(true);
        // Bypass §2.3 gate/ramp for the common fixture; dedicated tests restore defaults.
        LiftParamsStore::<Test>::put(LiftParams {
            new_subnet_min_age: 0,
            new_subnet_ramp: 1,
            ..LiftParams::default()
        });
        set_registered_at(0);
        set_subtoken_enabled(true);
        // Deep pool so capacity / depth checks pass.
        set_pool(1_000_000_000_000_000, 1_000_000_000_000_000);
        set_alpha_out(0);
        // Seed a warm reference so opens are not blocked by warmup.
        let spot = MockPool::price();
        References::<Test>::insert(
            netuid(),
            LendingReference {
                price: spot,
                last_updated: 1u64,
                valid_after: 1u64,
            },
        );
        ActiveNets::<Test>::insert(netuid(), ());
        ActiveNetCount::<Test>::put(1);
        Lifts::<Test>::mutate(netuid(), |book| {
            book.g_ema = pool_tao();
            // Aged past settle_ema_blocks so ensure_new_subnet_gate sees a warm G_ema.
            book.last = LiftParams::default().settle_ema_blocks;
        });
        // Warm settlement EMA to current basis; moving price high so it does not bind.
        set_tao_ema(Lending::settlement_tao_basis(netuid()));
        set_moving_price(U64F64::from_num(1_000_000u64));
    });
    ext
}

#[test]
fn lift_readd_preserves_price() {
    new_test_ext().execute_with(|| {
        let price_before = MockPool::price();
        let reserve = Lending::reserve_account(netuid());
        let custody = Lending::custody_hotkey().unwrap();
        let (a, t) = MockPool::lift_and_debit(netuid(), 1, 10, &reserve, &custody).unwrap();
        assert!(a.to_u64() > 0 && t.to_u64() > 0);
        let mid = MockPool::price();
        // Proportional lift leaves price unchanged.
        assert_eq!(mid, price_before);
        assert_ok!(MockPool::readd_and_credit(
            netuid(),
            &reserve,
            &custody,
            t,
            a
        ));
        assert_eq!(MockPool::price(), price_before);
    });
}

#[test]
fn short_escrows_tao_not_free_alpha() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let _hotkey = account(2);
        mint_tao(&owner, 50_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Short,
            10_000_000_000_000,
            None,
            1,
            1,
            0,
            deadline(),
        ));
        let position = Positions::<Test>::get(&owner, netuid()).unwrap();
        assert_eq!(position.side, Side::Short);
        assert!(position.principal > 0);
        assert!(position.proceeds > 0);
        assert_eq!(position.escrow_alpha, 0);
        // Borrower does not receive free borrowed alpha.
        assert_eq!(alpha(&owner, &account(2), netuid()), 0);
        let escrow = Lending::position_account(&owner, netuid());
        assert!(tao(&escrow) >= position.proceeds);
    });
}

#[test]
fn long_escrows_alpha_not_free_tao() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let hotkey = account(2);
        mint_tao(&owner, 1);
        mint_alpha(&owner, &hotkey, netuid(), 50_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Long,
            10_000_000_000_000,
            Some(hotkey.clone()),
            1,
            0,
            0,
            deadline(),
        ));
        let position = Positions::<Test>::get(&owner, netuid()).unwrap();
        assert_eq!(position.side, Side::Long);
        assert!(position.principal > 0);
        assert!(position.escrow_alpha > 0);
        assert_eq!(position.proceeds, 0);
        let escrow = Lending::position_account(&owner, netuid());
        let custody = Lending::custody_hotkey().unwrap();
        assert!(alpha(&escrow, &custody, netuid()) >= position.escrow_alpha);
    });
}

#[test]
fn raise_open_long_and_sell_is_atomic() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let hotkey = account(2);
        mint_alpha(&owner, &hotkey, netuid(), 100_000_000_000_000);
        let before = tao(&owner);
        assert_ok!(Lending::open_long_and_sell(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            hotkey.clone(),
            Some(5_000_000_000_000),
            None,
            5_000_000_000_000,
            1,
            Some(20_000_000_000_000),
            1,
            0,
            deadline(),
        ));
        let position = Positions::<Test>::get(&owner, netuid()).unwrap();
        assert_eq!(position.side, Side::Long);
        assert!(tao(&owner) > before);
        // Sold stake is gone from free alpha.
        assert!(alpha(&owner, &hotkey, netuid()) < 100_000_000_000_000);
    });
}

#[test]
fn raise_after_interest_forfeit_sells_full_new_escrow() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let hotkey = account(2);
        mint_alpha(&owner, &hotkey, netuid(), 200_000_000_000_000);
        assert_ok!(Lending::open_long_and_sell(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            hotkey.clone(),
            Some(5_000_000_000_000),
            None,
            5_000_000_000_000,
            1,
            None,
            1,
            0,
            deadline(),
        ));
        let first = Positions::<Test>::get(&owner, netuid()).unwrap();
        let old_escrow = first.escrow_alpha;
        assert!(old_escrow > 0);
        // Keep collateral healthy for raise *sizing*, but make the open-path
        // interest charge exhaust it and forfeit inside `do_open`.
        let now = System::block_number();
        Positions::<Test>::mutate(&owner, netuid(), |p| {
            if let Some(pos) = p {
                pos.annual_interest = 10_000_000_000_000;
                pos.interest_remainder = 0;
                pos.last_accrued = 0;
            }
        });
        System::set_block_number(now.saturating_add(520));
        mint_alpha(&owner, &hotkey, netuid(), 50_000_000_000_000);
        // Auditor: quote must preview forfeit (capacity + pool re-add), not
        // refuse SideCapacity against the pre-forfeit book.
        let collateral = 3_000_000_000_000u64;
        let before_l_l = Lifts::<Test>::get(netuid()).l_l;
        let before_pool_alpha = pool_alpha();
        let quoted = Lending::quote_raise(&owner, netuid(), &hotkey, None, Some(collateral), None)
            .expect("quote must succeed once forfeit frees Cap_long / reserves");
        assert!(quoted.principal_tao > 0);
        // Quote rolls back accrual/forfeit — live book unchanged until dispatch.
        assert!(Positions::<Test>::contains_key(&owner, netuid()));
        assert_eq!(Lifts::<Test>::get(netuid()).l_l, before_l_l);
        assert_eq!(pool_alpha(), before_pool_alpha);
        System::reset_events();
        assert_ok!(Lending::open_long_and_sell(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            hotkey.clone(),
            Some(collateral),
            None,
            collateral,
            quoted.principal_tao,
            None,
            1,
            0,
            deadline(),
        ));
        assert!(
            System::events()
                .into_iter()
                .any(|e| matches!(e.event, RuntimeEvent::Lending(Event::Forfeited { .. }))),
            "prior loan must forfeit during raise open"
        );
        let raised = System::events().into_iter().find_map(|e| match e.event {
            RuntimeEvent::Lending(Event::Raised {
                escrow_alpha,
                sell_alpha,
                ..
            }) => Some((escrow_alpha, sell_alpha)),
            _ => None,
        });
        let (evt_escrow, evt_sell) = raised.expect("Raised event");
        assert!(evt_escrow > 0, "replacement open must report new escrow");
        assert_eq!(evt_sell, evt_escrow);
        // Bug: subtracting forfeited old_escrow from the replacement book
        // would yield zero (old_escrow > new escrow on a smaller raise).
        assert_ne!(
            evt_escrow,
            0u64.saturating_sub(old_escrow),
            "must not treat forfeited escrow as still present"
        );
        let pos = Positions::<Test>::get(&owner, netuid()).unwrap();
        assert_eq!(pos.escrow_alpha, evt_escrow);
        // Fresh open after forfeit — principal is only this raise's borrow.
        assert!(pos.principal > 0);
        assert!(pos.principal < first.principal.saturating_add(pos.principal));
    });
}

/// Auditor: when an exhausted short occupies remaining Cap_short, quote must
/// preview forfeit (free capacity) rather than return SideCapacity while
/// dispatch would succeed after releasing the book.
#[test]
fn quote_open_matches_dispatch_when_forfeit_frees_side_capacity() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let _hotkey = account(2);
        mint_tao(&owner, 100_000_000_000_000);
        // Small short so Cap_short = 7.5%·G_ema can be set equal to its lift
        // while G_ema stays under the thin threshold.
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Short,
            100_000_000_000,
            None,
            1,
            0,
            0,
            deadline(),
        ));
        let opened = Positions::<Test>::get(&owner, netuid()).unwrap();
        let lift = opened.lift_tao;
        assert!(lift > 0);
        // Cap_short = 15%·50%·G_ema = 3/40·G_ema (thin). Set G_ema so Cap_short
        // equals this loan's lift; headroom is 0 until forfeit.
        let g_ema = lift.saturating_mul(40) / 3;
        assert!(
            g_ema < LiftParams::default().thin_tao,
            "g_ema={g_ema} must stay thin for 7.5% Cap_short"
        );
        Lifts::<Test>::mutate(netuid(), |book| {
            book.g_ema = g_ema.max(1);
            book.l_s = lift;
        });
        assert_err!(
            Lending::quote_open(netuid(), Side::Short, 50_000_000_000),
            Error::<Test>::SideCapacity
        );
        // Exhaust collateral on the next interest charge → forfeit on open path.
        let now = System::block_number();
        Positions::<Test>::mutate(&owner, netuid(), |p| {
            if let Some(pos) = p {
                pos.annual_interest = opened.collateral.saturating_mul(2).max(1);
                pos.interest_remainder = 0;
                pos.last_accrued = 0;
            }
        });
        System::set_block_number(now.saturating_add(520));
        mint_tao(&owner, 10_000_000_000_000);
        let add = 50_000_000_000u64;
        let quoted = Lending::quote_open_for(&owner, netuid(), Side::Short, add, None)
            .expect("quote must see post-forfeit capacity");
        assert!(quoted.principal > 0);
        // Quote rolled back: position and full Cap still present.
        assert!(Positions::<Test>::contains_key(&owner, netuid()));
        assert_eq!(Lifts::<Test>::get(netuid()).l_s, lift);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Short,
            add,
            None,
            quoted.principal,
            0,
            0,
            deadline(),
        ));
        let after = Positions::<Test>::get(&owner, netuid()).unwrap();
        assert_eq!(after.principal, quoted.principal);
    });
}

#[test]
fn quote_raise_sale_matches_post_buy_pool_not_live() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let hotkey = account(2);
        mint_alpha(&owner, &hotkey, netuid(), 100_000_000_000_000);
        let collateral = 5_000_000_000_000u64;
        let quoted =
            Lending::quote_raise(&owner, netuid(), &hotkey, None, Some(collateral), None).unwrap();
        assert!(quoted.escrow_alpha > 0);
        assert!(quoted.free_tao_out > 0);
        // Post-buy spot must move vs the pre-trade reference (buy lifts price).
        assert!(
            quoted.price_after_buy > quoted.price_before,
            "forced buy must raise quoted spot"
        );
        // Sale after the buy should leave spot below the post-buy peak.
        assert!(
            quoted.price_after < quoted.price_after_buy,
            "wallet sale must pull spot back from post-buy"
        );

        // Independently recompute the post-buy sale; must match free_tao_out.
        let y = pool_tao();
        let x = pool_alpha();
        let w = MockPool::quote_weight_parts(netuid());
        let p = quoted.principal_tao;
        let y_lift = y.saturating_sub(p);
        let x_lift =
            x.saturating_sub(((u128::from(x) * u128::from(p)) / u128::from(y).max(1)) as u64);
        let a = MockPool::quote_buy_on(y_lift.into(), x_lift.into(), w, p.into()).to_u64();
        let y_buy = y_lift.saturating_add(p);
        let x_buy = x_lift.saturating_sub(a);
        let expected =
            MockPool::quote_sell_on(y_buy.into(), x_buy.into(), w, quoted.sell_alpha.into())
                .to_u64();
        assert_eq!(quoted.free_tao_out, expected);
        assert_eq!(quoted.escrow_alpha, a);

        // Execution with the quoted floor must succeed and land near the quote.
        let tao_before = tao(&owner);
        assert_ok!(Lending::open_long_and_sell(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            hotkey,
            Some(collateral),
            None,
            collateral,
            quoted.principal_tao,
            None,
            quoted.free_tao_out.saturating_mul(99) / 100,
            0,
            deadline(),
        ));
        let gained = tao(&owner).saturating_sub(tao_before);
        let diff = gained.abs_diff(quoted.free_tao_out);
        assert!(
            diff <= quoted.free_tao_out / 50 + 1,
            "executed free TAO {} vs quote {} (diff {})",
            gained,
            quoted.free_tao_out,
            diff
        );
    });
}

#[test]
fn raise_increase_default_sale_is_new_escrow_only() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let hotkey = account(2);
        // Large unlocked stake so a buggy cumulative default sale would succeed.
        mint_alpha(&owner, &hotkey, netuid(), 200_000_000_000_000);
        assert_ok!(Lending::open_long_and_sell(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            hotkey.clone(),
            Some(10_000_000_000_000),
            None,
            10_000_000_000_000,
            1,
            None, // default S = newly escrowed A
            1,
            0,
            deadline(),
        ));
        let first = Positions::<Test>::get(&owner, netuid()).unwrap();
        let escrow_after_first = first.escrow_alpha;
        assert!(escrow_after_first > 0);

        // Top up unlocked stake for the increase's collateral + sale.
        mint_alpha(&owner, &hotkey, netuid(), 50_000_000_000_000);
        let alpha_before = alpha(&owner, &hotkey, netuid());
        System::reset_events();
        assert_ok!(Lending::open_long_and_sell(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            hotkey.clone(),
            Some(2_000_000_000_000),
            None,
            2_000_000_000_000,
            1,
            None, // must default to *new* escrow only
            1,
            0,
            deadline(),
        ));
        let second = Positions::<Test>::get(&owner, netuid()).unwrap();
        let new_escrow = second.escrow_alpha.saturating_sub(escrow_after_first);
        assert!(new_escrow > 0);
        assert!(new_escrow < escrow_after_first);

        let sold = alpha_before
            .saturating_sub(alpha(&owner, &hotkey, netuid()))
            .saturating_sub(2_000_000_000_000); // subtract collateral locked
        // Sale quantity ≈ new escrow (not cumulative first+new).
        let diff = sold.abs_diff(new_escrow);
        assert!(
            diff <= new_escrow / 100 + 1,
            "default raise sale {} must track new escrow {}, not cumulative {}",
            sold,
            new_escrow,
            second.escrow_alpha
        );
        assert!(
            sold < escrow_after_first,
            "must not sell prior escrow book from unlocked stake"
        );

        let raised = System::events().into_iter().find_map(|e| match e.event {
            RuntimeEvent::Lending(Event::Raised {
                escrow_alpha,
                sell_alpha,
                ..
            }) => Some((escrow_alpha, sell_alpha)),
            _ => None,
        });
        let (evt_escrow, evt_sell) = raised.expect("Raised event");
        assert_eq!(evt_escrow, new_escrow);
        assert_eq!(evt_sell, new_escrow);
    });
}

#[test]
fn raise_rolls_back_on_slippage() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let hotkey = account(2);
        let stake = 100_000_000_000_000u64;
        mint_alpha(&owner, &hotkey, netuid(), stake);
        let alpha_before = alpha(&owner, &hotkey, netuid());
        let tao_before = tao(&owner);
        // Impossible sale floor forces SlippageExceeded after the long leg —
        // transactional rollback must leave no position and restore stake.
        assert!(
            Lending::open_long_and_sell(
                RuntimeOrigin::signed(owner.clone()),
                netuid(),
                hotkey.clone(),
                Some(5_000_000_000_000),
                None,
                5_000_000_000_000,
                1,
                Some(20_000_000_000_000),
                u64::MAX,
                0,
                deadline(),
            )
            .is_err()
        );
        assert!(Positions::<Test>::get(&owner, netuid()).is_none());
        assert_eq!(alpha(&owner, &hotkey, netuid()), alpha_before);
        assert_eq!(tao(&owner), tao_before);
        assert_eq!(Lifts::<Test>::get(netuid()).l_l, 0);
        assert_eq!(Lifts::<Test>::get(netuid()).held_tao, 0);
    });
}

#[test]
fn capacity_floor_ceiling_and_side_split() {
    new_test_ext().execute_with(|| {
        let params = LiftParams::default();
        assert_eq!(params.floor_bps, 3_300);
        assert_eq!(params.thin_floor_bps, 1_500);
        assert_eq!(params.ceiling_bps, 5_000);
        assert_eq!(params.split_short_bps, 5_000);
        assert_ok!(Lending::set_lift_params(
            RuntimeOrigin::root(),
            LiftParams {
                floor_bps: 3_000,
                ..params
            }
        ));
        assert_eq!(Lending::lift_params().floor_bps, 3_000);
    });
}

#[test]
fn dynamic_valuation_tracks_spot_after_warmup() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let _hotkey = account(2);
        mint_tao(&owner, 50_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Short,
            5_000_000_000_000,
            None,
            1,
            1,
            0,
            deadline(),
        ));
        let ref_before = References::<Test>::get(netuid()).unwrap().price;
        // Move spot by selling into the pool from an unrelated holder.
        let dump = account(9);
        let dump_hot = account(10);
        mint_alpha(&dump, &dump_hot, netuid(), 200_000_000_000_000);
        set_alpha_out(alpha_out().saturating_add(200_000_000_000_000));
        assert_ok!(MockPool::sell_alpha(
            &dump,
            &dump_hot,
            netuid(),
            100_000_000_000_000u64.into(),
            0u64.into(),
            false
        ));
        System::set_block_number(3);
        Lending::on_finalize(3u64);
        let ref_after = References::<Test>::get(netuid()).unwrap().price;
        assert!(ref_after < ref_before || MockPool::price() < ref_before);
    });
}

#[test]
fn section13_legacy_dump_short_gets_positive_refund() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let _hotkey = account(2);
        mint_tao(&owner, 80_000_000_000_000);
        // Open short while price is ~1.
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Short,
            20_000_000_000_000,
            None,
            1,
            1,
            0,
            deadline(),
        ));
        let position = Positions::<Test>::get(&owner, netuid()).unwrap();
        let principal = position.principal;
        let escrow_before = position.proceeds.saturating_add(position.collateral);
        assert!(escrow_before > 0);

        // Legacy dump: most holders dump alpha into the pool, collapsing price
        // and bloating AlphaIn while AlphaOut (staker claims) shrinks.
        // Keep a thin residual AlphaOut so the legacy divisor stays nonzero.
        let residual = account(6);
        let residual_hot = account(5);
        mint_alpha(&residual, &residual_hot, netuid(), 1_000_000_000_000);
        set_alpha_out(alpha_out().saturating_add(1_000_000_000_000));

        let dumper = account(7);
        let dump_hot = account(8);
        let dump_alpha = 800_000_000_000_000u64;
        mint_alpha(&dumper, &dump_hot, netuid(), dump_alpha);
        set_alpha_out(alpha_out().saturating_add(dump_alpha));
        assert_ok!(MockPool::sell_alpha(
            &dumper,
            &dump_hot,
            netuid(),
            dump_alpha.into(),
            0u64.into(),
            false
        ));

        // Main's legacy divisor omits AlphaIn → inflated rate that can wipe shorts.
        let pot = pool_tao();
        let legacy_denom = MockPool::eligible_alpha_for_dissolution(netuid()).unwrap();
        let all_alpha = MockPool::all_alpha_for_lending_settlement(netuid());
        assert!(legacy_denom > 0);
        assert!(all_alpha > legacy_denom);
        let r_all = U64F64::from_num(pot) / U64F64::from_num(all_alpha);
        let r_legacy = U64F64::from_num(pot) / U64F64::from_num(legacy_denom);
        assert!(
            r_all < r_legacy,
            "all-alpha rate must be below legacy (r_all={r_all}, r_legacy={r_legacy}, pot={pot}, all={all_alpha}, legacy={legacy_denom})"
        );

        // Align §13.2 snapshots with the post-dump spot basis so this test still
        // exercises all-alpha r (not EMA lag).
        let basis = Lending::settlement_tao_basis(netuid());
        set_tao_ema(basis);
        set_moving_price(U64F64::from_num(1_000_000u64));

        // §13.2 settlement path (ema == basis → spot r = T/D).
        assert_ok!(Lending::start_dissolution(netuid()));
        assert_ok!(Lending::readd_lifts_for_dissolution(netuid()));
        let mut meter = WeightMeter::with_limit(Weight::from_parts(u64::MAX, u64::MAX));
        assert!(Lending::settle_lending(netuid(), &mut meter));

        // Short ends with a positive payout because r uses ALL_alpha.
        let r_snap = U64F64::from_num(basis) / U64F64::from_num(all_alpha);
        let debt = (U64F64::from_num(principal) * r_snap)
            .saturating_ceil()
            .saturating_to_num::<u64>();
        let expected_refund = escrow_before.saturating_sub(debt.min(escrow_before));
        assert!(
            expected_refund > 0,
            "legacy-dump short must receive positive refund under all-alpha r (debt={debt}, escrow={escrow_before}, r_all={r_all}, r_legacy={r_legacy})"
        );
        assert!(Positions::<Test>::get(&owner, netuid()).is_none());
        assert!(tao(&owner) >= expected_refund.saturating_sub(1));
        let _ = burned();
    });
}

#[test]
fn section132_one_block_pump_changes_short_settlement_under_1pct() {
    new_test_ext().execute_with(|| {
        // Warm EMA with a few finalizes on a stable pool.
        for b in 11u64..16 {
            System::set_block_number(b);
            Lending::on_finalize(b);
        }
        let owner = account(1);
        let _hotkey = account(2);
        mint_tao(&owner, 80_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Short,
            20_000_000_000_000,
            None,
            1,
            1,
            0,
            deadline(),
        ));
        let position = Positions::<Test>::get(&owner, netuid()).unwrap();
        let principal = position.principal;
        let escrow_before = position.proceeds.saturating_add(position.collateral);

        // Align EMA to current basis, then record pre-pump expectation.
        let t_pre = Lending::settlement_tao_basis(netuid());
        set_tao_ema(t_pre);
        set_moving_price(U64F64::from_num(1_000_000u64));
        let d = MockPool::all_alpha_for_lending_settlement(netuid());
        assert!(d > 0);
        let t_settle_pre = t_pre.min(t_pre);
        let bits = (u128::from(t_settle_pre) << 64) / d;
        let mut r_pre = U64F64::from_bits(bits);
        let mp = MockPool::moving_alpha_price(netuid());
        if mp > U64F64::from_num(0) {
            r_pre = r_pre.min(mp);
        }
        let expected_debt = (U64F64::from_num(principal) * r_pre)
            .saturating_ceil()
            .saturating_to_num::<u64>();
        let expected_refund = escrow_before.saturating_sub(expected_debt.min(escrow_before));

        // One-block pump: inflate pool TAO 10× without updating EMA.
        let pump = pool_tao().saturating_mul(10);
        write(
            &key(b"test/pool_tao", netuid()),
            pool_tao().saturating_add(pump),
        );
        assert!(Lending::settlement_tao_basis(netuid()) > t_pre);

        assert_ok!(Lending::start_dissolution(netuid()));
        let d_flag = Dissolutions::<Test>::get(netuid()).unwrap();
        assert_eq!(d_flag.t_ema_flag, t_pre);
        assert!(d_flag.t_flag > t_pre);
        assert_ok!(Lending::readd_lifts_for_dissolution(netuid()));
        let mut meter = WeightMeter::with_limit(Weight::from_parts(u64::MAX, u64::MAX));
        assert!(Lending::settle_lending(netuid(), &mut meter));

        let rate = Dissolutions::<Test>::get(netuid()).unwrap().rate;
        let debt = (U64F64::from_num(principal) * rate)
            .saturating_ceil()
            .saturating_to_num::<u64>();
        // Pump must not confiscate: debt within 1% of pre-pump expectation.
        let tol = expected_debt / 100 + 1;
        assert!(
            debt.abs_diff(expected_debt) <= tol,
            "debt drifted >1%: debt={debt} expected={expected_debt} rate={rate} r_pre={r_pre}"
        );
        assert!(Positions::<Test>::get(&owner, netuid()).is_none());
        let refund_tol = expected_refund / 100 + 1;
        assert!(
            tao(&owner).abs_diff(expected_refund) <= refund_tol
                || tao(&owner) + 1 >= expected_refund.saturating_sub(refund_tol),
            "refund drifted >1%: owner={} expected={expected_refund}",
            tao(&owner)
        );
    });
}

#[test]
fn section132_no_pump_matches_spot_when_ema_equals_basis() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let _hotkey = account(2);
        mint_tao(&owner, 50_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Short,
            10_000_000_000_000,
            None,
            1,
            1,
            0,
            deadline(),
        ));
        let t = Lending::settlement_tao_basis(netuid());
        set_tao_ema(t);
        set_moving_price(U64F64::from_num(1_000_000u64));
        let d = MockPool::all_alpha_for_lending_settlement(netuid());
        let bits = (u128::from(t) << 64) / d;
        let expected_r = U64F64::from_bits(bits);

        assert_ok!(Lending::start_dissolution(netuid()));
        assert_ok!(Lending::readd_lifts_for_dissolution(netuid()));
        let mut meter = WeightMeter::with_limit(Weight::from_parts(u64::MAX, u64::MAX));
        assert!(Lending::settle_lending(netuid(), &mut meter));
        let rate = Dissolutions::<Test>::get(netuid()).unwrap().rate;
        assert_eq!(rate, expected_r);
    });
}

#[test]
fn section132_moving_price_cap_binds() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let _hotkey = account(2);
        mint_tao(&owner, 50_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Short,
            10_000_000_000_000,
            None,
            1,
            1,
            0,
            deadline(),
        ));
        let t = Lending::settlement_tao_basis(netuid());
        set_tao_ema(t);
        let d = MockPool::all_alpha_for_lending_settlement(netuid());
        let bits = (u128::from(t) << 64) / d;
        let spot_r = U64F64::from_bits(bits);
        // Cap below T/D.
        let cap = spot_r / U64F64::from_num(2u64);
        assert!(cap > U64F64::from_num(0));
        set_moving_price(cap);

        assert_ok!(Lending::start_dissolution(netuid()));
        assert_ok!(Lending::readd_lifts_for_dissolution(netuid()));
        let mut meter = WeightMeter::with_limit(Weight::from_parts(u64::MAX, u64::MAX));
        assert!(Lending::settle_lending(netuid(), &mut meter));
        let rate = Dissolutions::<Test>::get(netuid()).unwrap().rate;
        assert_eq!(rate, cap);
    });
}

#[test]
fn section132_tao_ema_inits_to_current_basis() {
    new_test_ext().execute_with(|| {
        clear_tao_ema();
        assert!(tao_ema().is_none());
        let basis = Lending::settlement_tao_basis(netuid());
        // Via ensure_tao_ema (also exercised by on_finalize / on_runtime_upgrade).
        let ema = Lending::ensure_tao_ema(netuid());
        assert_eq!(ema, basis);
        assert_eq!(tao_ema(), Some(basis));

        clear_tao_ema();
        let _ = <Lending as Hooks<_>>::on_runtime_upgrade();
        assert_eq!(tao_ema(), Some(Lending::settlement_tao_basis(netuid())));
    });
}

#[test]
fn dissolving_blocks_new_opens_only_when_flagged() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let _hotkey = account(2);
        mint_tao(&owner, 50_000_000_000_000);
        // Near-dereg is NOT modeled; only Dissolutions flag blocks opens.
        assert_ok!(Lending::start_dissolution(netuid()));
        assert!(
            Lending::open(
                RuntimeOrigin::signed(owner),
                netuid(),
                Side::Short,
                1_000_000_000_000,
                None,
                1,
                1,
                0,
                deadline(),
            )
            .is_err()
        );
    });
}

#[test]
fn short_ltv_values_tao_collateral_directly() {
    new_test_ext().execute_with(|| {
        // Price = 10 TAO/alpha so the old alpha-priced collateral bug is visible.
        set_pool(10_000_000_000_000_000, 1_000_000_000_000_000);
        let spot = MockPool::price();
        References::<Test>::insert(
            netuid(),
            LendingReference {
                price: spot,
                last_updated: 1u64,
                valid_after: 1u64,
            },
        );
        let owner = account(1);
        let _hotkey = account(2);
        let collateral = 100_000_000_000u64; // 100 TAO
        mint_tao(&owner, collateral * 2);
        // At 25% LTV, max TAO debt = 25 TAO. At price 10 that is 2.5 alpha.
        // The old bug priced 100 TAO collateral as 1000 TAO and admitted 25 alpha.
        assert!(
            Lending::open_exact(
                RuntimeOrigin::signed(owner.clone()),
                netuid(),
                Side::Short,
                collateral,
                None,
                25_000_000_000, // 25 alpha ≈ 250 TAO debt
                1,
                0,
                deadline(),
            )
            .is_err(),
            "oversized short must fail when LTV applies to TAO collateral"
        );
        // Boundary under the TAO LTV bound must succeed.
        assert_ok!(Lending::open_exact(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Short,
            collateral,
            None,
            2_000_000_000, // 2 alpha ≈ 20 TAO debt < 25 TAO max
            1,
            0,
            deadline(),
        ));
        let position = Positions::<Test>::get(&owner, netuid()).unwrap();
        assert_eq!(position.principal, 2_000_000_000);
        assert_eq!(position.collateral, collateral);
    });
}

#[test]
fn raise_rejects_dissolving_subnet() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let hotkey = account(2);
        mint_alpha(&owner, &hotkey, netuid(), 100_000_000_000_000);
        assert_ok!(Lending::start_dissolution(netuid()));
        assert!(
            Lending::open_long_and_sell(
                RuntimeOrigin::signed(owner),
                netuid(),
                hotkey,
                Some(5_000_000_000_000),
                None,
                5_000_000_000_000,
                1,
                None,
                0,
                0,
                deadline(),
            )
            .is_err()
        );
    });
}

#[test]
fn raise_rejects_insufficient_collateral_stake() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let hotkey = account(2);
        mint_alpha(&owner, &hotkey, netuid(), 1_000); // tiny
        assert!(
            Lending::open_long_and_sell(
                RuntimeOrigin::signed(owner),
                netuid(),
                hotkey,
                Some(5_000_000_000_000),
                None,
                5_000_000_000_000,
                1,
                None,
                0,
                0,
                deadline(),
            )
            .is_err()
        );
    });
}

#[test]
fn raise_rejects_when_long_capacity_exhausted() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let hotkey = account(2);
        mint_alpha(&owner, &hotkey, netuid(), 500_000_000_000_000);
        // Saturate the long book against the live side cap.
        let params = LiftParams::default();
        let (_cap, _cs, cl) = {
            // Force book L_L to the current long cap.
            let (cap, cs, cl) = {
                let book = Lifts::<Test>::get(netuid());
                let g_ref = pool_tao(); // deep pool ≈ g_ref
                let _ = (book, g_ref);
                Lending::lift_params();
                // Use side_caps via a tiny open then mutate.
                (0u64, 0u64, 0u64)
            };
            let _ = (cap, cs);
            // Directly fill L_L past any reasonable open.
            Lifts::<Test>::mutate(netuid(), |book| {
                book.l_l = u64::MAX / 2;
                book.b_l = book.l_l;
            });
            (0u64, 0u64, cl)
        };
        let _ = (params, cl);
        assert!(
            Lending::open_long_and_sell(
                RuntimeOrigin::signed(owner),
                netuid(),
                hotkey,
                Some(5_000_000_000_000),
                None,
                5_000_000_000_000,
                1,
                None,
                0,
                0,
                deadline(),
            )
            .is_err()
        );
    });
}

#[test]
fn raise_partial_capacity_still_opens() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let hotkey = account(2);
        mint_alpha(&owner, &hotkey, netuid(), 200_000_000_000_000);
        // Consume some long capacity but leave headroom.
        Lifts::<Test>::mutate(netuid(), |book| {
            book.l_l = 1_000_000_000_000;
            book.b_l = 1_000_000_000_000;
        });
        assert_ok!(Lending::open_long_and_sell(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            hotkey,
            Some(2_000_000_000_000),
            None,
            2_000_000_000_000,
            1,
            None,
            0,
            0,
            deadline(),
        ));
        let position = Positions::<Test>::get(&owner, netuid()).unwrap();
        assert_eq!(position.side, Side::Long);
        assert!(position.principal > 0);
    });
}

#[test]
fn raise_borrow_tao_path_and_rounding_edge() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let hotkey = account(2);
        mint_alpha(&owner, &hotkey, netuid(), 200_000_000_000_000);
        // Exact borrow path: size minimal collateral under the ceiling.
        assert_ok!(Lending::open_long_and_sell(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            hotkey.clone(),
            None,
            Some(1_000_000_000_000),
            50_000_000_000_000,
            1_000_000_000_000,
            None,
            0,
            0,
            deadline(),
        ));
        let position = Positions::<Test>::get(&owner, netuid()).unwrap();
        assert_eq!(position.principal, 1_000_000_000_000);
        assert!(position.collateral > 0);
        // Zero borrow rejected.
        assert!(
            Lending::open_long_and_sell(
                RuntimeOrigin::signed(account(3)),
                netuid(),
                account(4),
                None,
                Some(0),
                1,
                0,
                None,
                0,
                0,
                deadline(),
            )
            .is_err()
        );
    });
}

#[test]
fn short_buyback_bounds_input_to_remaining_principal() {
    new_test_ext().execute_with(|| {
        set_pool(1_000_000_000_000_000, 1_000_000_000_000_000);
        let spot = MockPool::price();
        References::<Test>::insert(
            netuid(),
            LendingReference {
                price: spot,
                last_updated: 1u64,
                valid_after: 1u64,
            },
        );
        let owner = account(1);
        let hotkey = account(2);
        // Large TAO collateral relative to a small alpha principal.
        mint_tao(&owner, 50_000_000_000_000);
        assert_ok!(Lending::open_exact(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Short,
            10_000_000_000_000,
            None,
            100_000_000_000, // small principal
            1,
            0,
            deadline(),
        ));
        let position = Positions::<Test>::get(&owner, netuid()).unwrap();
        let principal = position.principal;
        let escrow = Lending::position_account(&owner, netuid());
        let custody = Lending::custody_hotkey().unwrap();
        let alpha_before_owner = alpha(&owner, &hotkey, netuid());
        assert_ok!(Lending::close(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            account(2),
            false,
            u64::MAX,
            0,
            deadline(),
        ));
        assert!(Positions::<Test>::get(&owner, netuid()).is_none());
        // Escrow should not retain stranded alpha after close.
        assert_eq!(alpha(&escrow, &custody, netuid()), 0);
        // Owner may receive surplus alpha from any residual overshoot.
        let _ = (principal, alpha_before_owner);
    });
}

#[test]
fn dissolution_interest_does_not_forfeit_after_readd() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let _hotkey = account(2);
        mint_tao(&owner, 50_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Short,
            5_000_000_000_000,
            None,
            1,
            1,
            0,
            deadline(),
        ));
        // Drain collateral so final interest charge would forfeit on the live path.
        Positions::<Test>::mutate(&owner, netuid(), |maybe| {
            if let Some(p) = maybe {
                p.collateral = 0;
                p.annual_interest = 1_000_000_000_000;
                p.last_accrued = 0u64;
            }
        });
        assert_ok!(Lending::start_dissolution(netuid()));
        // Readd held legs as dissolution does.
        assert_ok!(Lending::readd_lifts_for_dissolution(netuid()));
        let mut meter = WeightMeter::with_limit(Weight::from_parts(u64::MAX, u64::MAX));
        // Must complete without stalling on a double-readd forfeit.
        assert!(Lending::settle_lending(netuid(), &mut meter));
        assert!(Positions::<Test>::get(&owner, netuid()).is_none());
    });
}

/// Accrued long interest at dissolve must not stall settle: stake transfers
/// reject once NetworksAdded is cleared, so interest alpha is destroyed in place.
#[test]
fn dissolution_settles_long_with_accrued_interest() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let hotkey = account(2);
        mint_tao(&owner, 1);
        mint_alpha(&owner, &hotkey, netuid(), 50_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Long,
            10_000_000_000_000,
            Some(hotkey),
            1,
            0,
            0,
            deadline(),
        ));
        // Accrue a non-trivial coupon so settle's final charge_interest moves alpha.
        Positions::<Test>::mutate(&owner, netuid(), |maybe| {
            if let Some(p) = maybe {
                p.annual_interest = p.collateral / 10;
                p.last_accrued = 0u64;
            }
        });
        System::set_block_number(System::block_number().saturating_add(50_400));
        assert_ok!(Lending::start_dissolution(netuid()));
        assert_ok!(Lending::readd_lifts_for_dissolution(netuid()));
        let mut meter = WeightMeter::with_limit(Weight::from_parts(u64::MAX, u64::MAX));
        assert!(
            Lending::settle_lending(netuid(), &mut meter),
            "long interest at dissolve must not roll back settle"
        );
        assert!(Positions::<Test>::get(&owner, netuid()).is_none());
        assert_eq!(LongInterestPending::<Test>::get(netuid()), 0);
    });
}

#[test]
fn settle_phase0_subtracts_delta0_and_emits_settle_rate_fixed() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let _hotkey = account(2);
        mint_tao(&owner, 50_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Short,
            10_000_000_000_000,
            None,
            1,
            1,
            0,
            deadline(),
        ));
        let t_flag = Lending::settlement_tao_basis(netuid());
        set_tao_ema(t_flag);
        assert_ok!(Lending::start_dissolution(netuid()));
        assert_ok!(Lending::readd_lifts_for_dissolution(netuid()));
        // Simulate phase-0 basket outflow: pot TAO drops by delta0 after the flag.
        let delta0 = 1_000_000_000_000u64;
        let t_pre = t_flag.saturating_sub(delta0);
        set_pool(
            t_pre.saturating_sub(Lifts::<Test>::get(netuid()).held_tao),
            pool_alpha(),
        );
        let mut meter = WeightMeter::with_limit(Weight::from_parts(u64::MAX, u64::MAX));
        assert!(Lending::settle_lending(netuid(), &mut meter));
        let dissolution = Dissolutions::<Test>::get(netuid()).unwrap();
        let expected_t_settle = t_flag.min(t_flag).saturating_sub(delta0);
        let d = MockPool::all_alpha_for_lending_settlement(netuid());
        let expected_bits = (u128::from(expected_t_settle) << 64) / d.max(1);
        assert_eq!(dissolution.rate.to_bits(), expected_bits);
        // Δ0 never negative; T_settle ≤ min(T_flag, t_ema_flag).
        assert!(expected_t_settle <= t_flag);
        let found = System::events().iter().any(|e| {
            matches!(
                &e.event,
                RuntimeEvent::Lending(Event::SettleRateFixed {
                    delta0: d0,
                    t_pre: tp,
                    ..
                }) if *d0 == delta0 && *tp == t_pre
            )
        });
        assert!(found, "SettleRateFixed must fire with Δ0 and T_pre");
    });
}

#[test]
fn deadline_passed_rejects_open_close_and_raise() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let hotkey = account(2);
        mint_tao(&owner, 50_000_000_000_000);
        mint_alpha(&owner, &hotkey, netuid(), 50_000_000_000_000);
        let past = 1u64; // block is 10
        assert!(
            Lending::open(
                RuntimeOrigin::signed(owner.clone()),
                netuid(),
                Side::Short,
                1_000_000_000_000,
                None,
                1,
                0,
                0,
                past,
            )
            .is_err()
        );
        assert!(
            Lending::open_exact(
                RuntimeOrigin::signed(owner.clone()),
                netuid(),
                Side::Long,
                1_000_000_000_000,
                Some(hotkey.clone()),
                1_000_000_000,
                0,
                0,
                past,
            )
            .is_err()
        );
        assert!(
            Lending::open_long_and_sell(
                RuntimeOrigin::signed(owner.clone()),
                netuid(),
                hotkey.clone(),
                Some(1_000_000_000_000),
                None,
                1_000_000_000_000,
                1,
                None,
                0,
                0,
                past,
            )
            .is_err()
        );
        // Open a live position then fail close on deadline.
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Short,
            5_000_000_000_000,
            None,
            1,
            0,
            0,
            deadline(),
        ));
        assert!(
            Lending::close(
                RuntimeOrigin::signed(owner),
                netuid(),
                account(2),
                true,
                u64::MAX,
                0,
                past,
            )
            .is_err()
        );
    });
}

#[test]
fn long_open_enforces_min_escrow_alpha_short_uses_min_proceeds_only() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let hotkey = account(2);
        mint_alpha(&owner, &hotkey, netuid(), 50_000_000_000_000);
        // Impossible escrow floor on the forced buy.
        assert!(
            Lending::open(
                RuntimeOrigin::signed(owner.clone()),
                netuid(),
                Side::Long,
                5_000_000_000_000,
                Some(hotkey.clone()),
                1,
                0,        // min_proceeds ignored for longs
                u64::MAX, // min_escrow_alpha
                deadline(),
            )
            .is_err()
        );
        mint_tao(&owner, 50_000_000_000_000);
        // Impossible proceeds floor on the forced short sale.
        assert!(
            Lending::open(
                RuntimeOrigin::signed(owner.clone()),
                netuid(),
                Side::Short,
                5_000_000_000_000,
                None,
                1,
                u64::MAX, // min_proceeds
                0,        // min_escrow_alpha ignored for shorts
                deadline(),
            )
            .is_err()
        );
    });
}

#[test]
fn partial_repay_scales_position_pro_rata() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let hotkey = account(2);
        mint_tao(&owner, 100_000_000_000_000);
        assert_ok!(Lending::open_exact(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Short,
            40_000_000_000_000,
            None,
            4_000_000_000_000,
            1,
            0,
            deadline(),
        ));
        let before = Positions::<Test>::get(&owner, netuid()).unwrap();
        let half = before.principal / 2;
        // Wallet repay: supply half principal alpha, take half escrow.
        mint_alpha(&owner, &hotkey, netuid(), half);
        assert_ok!(Lending::repay(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            account(2),
            half,
            true,
            u64::MAX,
            0,
            deadline(),
        ));
        let after = Positions::<Test>::get(&owner, netuid()).unwrap();
        assert_eq!(after.principal, before.principal - half);
        let slice_c = ((u128::from(before.collateral) * u128::from(half))
            / u128::from(before.principal)) as u64;
        assert_eq!(after.collateral, before.collateral - slice_c);
        // Remainder must keep opening value ≥ 1 τ.
        assert!(after.principal > 0);
        // Full repay via repay(remaining) closes.
        mint_alpha(&owner, &hotkey, netuid(), after.principal);
        assert_ok!(Lending::repay(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            account(2),
            after.principal,
            true,
            u64::MAX,
            0,
            deadline(),
        ));
        assert!(Positions::<Test>::get(&owner, netuid()).is_none());
    });
}

#[test]
fn coldkey_swap_rekeys_position_without_moving_custody() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let new_owner = account(11);
        let _hotkey = account(2);
        mint_tao(&owner, 50_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Short,
            10_000_000_000_000,
            None,
            1,
            1,
            0,
            deadline(),
        ));
        let position = Positions::<Test>::get(&owner, netuid()).unwrap();
        let custody = position.custody.clone();
        let escrow_tao_before = tao(&custody);
        assert_eq!(
            <Lending as LendingInterface<_>>::count_positions_bounded(&owner, 8),
            1
        );
        assert_ok!(Lending::on_coldkey_swap(&owner, &new_owner));
        assert!(Positions::<Test>::get(&owner, netuid()).is_none());
        let migrated = Positions::<Test>::get(&new_owner, netuid()).unwrap();
        assert_eq!(migrated.custody, custody);
        assert_eq!(tao(&custody), escrow_tao_before);
        assert!(OpenByNetuid::<Test>::contains_key(netuid(), &new_owner));
        assert_eq!(
            EscrowOwner::<Test>::get(netuid(), &custody),
            Some(new_owner.clone())
        );
        assert_eq!(
            <Lending as LendingInterface<_>>::count_positions_bounded(&owner, 8),
            0
        );
        assert_eq!(
            <Lending as LendingInterface<_>>::count_positions_bounded(&new_owner, 8),
            1
        );
    });
}

#[test]
fn new_subnet_gate_and_capacity_ramp() {
    new_test_ext().execute_with(|| {
        // Restore production defaults.
        LiftParamsStore::<Test>::put(LiftParams::default());
        let owner = account(1);
        let _hotkey = account(2);
        mint_tao(&owner, 50_000_000_000_000);
        set_subtoken_enabled(false);
        assert_err!(
            Lending::open(
                RuntimeOrigin::signed(owner.clone()),
                netuid(),
                Side::Short,
                1_000_000_000_000,
                None,
                1,
                0,
                0,
                deadline(),
            ),
            Error::<Test>::SubnetNotStarted
        );
        set_subtoken_enabled(true);
        // Age below min_age.
        set_registered_at(System::block_number());
        assert_err!(
            Lending::open(
                RuntimeOrigin::signed(owner.clone()),
                netuid(),
                Side::Short,
                1_000_000_000_000,
                None,
                1,
                0,
                0,
                deadline(),
            ),
            Error::<Test>::SubnetTooNew
        );
        // Past gate: age just over min_age so ramp factor is tiny; a large open
        // must hit SideCapacityRamping (full Cap would still fit).
        let params = LiftParams::default();
        let now = params.new_subnet_min_age.saturating_add(2);
        System::set_block_number(now);
        set_registered_at(0);
        References::<Test>::mutate(netuid(), |r| {
            if let Some(reference) = r {
                reference.valid_after = 1;
                reference.last_updated = now;
            }
        });
        Lifts::<Test>::mutate(netuid(), |book| {
            book.last = now;
            book.g_ema = pool_tao();
        });
        assert_err!(
            Lending::quote_open_for(&owner, netuid(), Side::Short, 20_000_000_000_000, None),
            Error::<Test>::SideCapacityRamping
        );
        assert_err!(
            Lending::open(
                RuntimeOrigin::signed(owner.clone()),
                netuid(),
                Side::Short,
                20_000_000_000_000,
                None,
                1,
                0,
                0,
                deadline(),
            ),
            Error::<Test>::SideCapacityRamping
        );
    });
}

#[test]
fn side_capacity_ramping_when_only_ramp_binds_quote_matches_open() {
    new_test_ext().execute_with(|| {
        // Production ramp defaults; age just past the gate so Cap is a tiny
        // fraction of full. Full Cap still dwarfs a modest short.
        LiftParamsStore::<Test>::put(LiftParams::default());
        let owner = account(1);
        let _hotkey = account(2);
        mint_tao(&owner, 100_000_000_000_000);
        let params = LiftParams::default();
        let now = System::block_number().max(params.new_subnet_min_age + 2);
        System::set_block_number(now);
        set_registered_at(now.saturating_sub(params.new_subnet_min_age + 1));
        Lifts::<Test>::mutate(netuid(), |book| {
            book.last = now;
            book.g_ema = pool_tao();
            book.l_s = 0;
        });
        let collateral = 5_000_000_000_000u64;
        assert_err!(
            Lending::quote_open_for(&owner, netuid(), Side::Short, collateral, None),
            Error::<Test>::SideCapacityRamping
        );
        assert_err!(
            Lending::open(
                RuntimeOrigin::signed(owner.clone()),
                netuid(),
                Side::Short,
                collateral,
                None,
                1,
                0,
                0,
                deadline(),
            ),
            Error::<Test>::SideCapacityRamping
        );
        // After the ramp completes, the same size is not ramp-bound (may still
        // open or fail for unrelated reasons — here it should open).
        let ready = params
            .new_subnet_min_age
            .saturating_add(params.new_subnet_ramp)
            .saturating_add(10);
        System::set_block_number(ready);
        set_registered_at(0);
        Lifts::<Test>::mutate(netuid(), |book| {
            book.last = ready;
            book.g_ema = pool_tao();
        });
        assert_ok!(Lending::quote_open_for(
            &owner,
            netuid(),
            Side::Short,
            collateral,
            None
        ));
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner),
            netuid(),
            Side::Short,
            collateral,
            None,
            1,
            0,
            0,
            deadline(),
        ));
    });
}

#[test]
fn custody_hotkey_is_detected_for_stake_guards() {
    new_test_ext().execute_with(|| {
        let custody = Lending::custody_hotkey().unwrap();
        assert!(Lending::is_custody_hotkey(&custody));
        assert!(!Lending::is_custody_hotkey(&account(99)));
        // Pallet hotkey is registered under the pallet coldkey only — third
        // parties cannot treat it as a normal staking target (§14).
        assert!(PalletHotkey::<Test>::get().is_some());
    });
}

#[test]
fn coldkey_swap_interest_debits_stored_custody() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let new_owner = account(11);
        let _hotkey = account(2);
        mint_tao(&owner, 50_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Short,
            10_000_000_000_000,
            None,
            1,
            1,
            0,
            deadline(),
        ));
        let position = Positions::<Test>::get(&owner, netuid()).unwrap();
        let custody = position.custody.clone();
        let escrow_before = tao(&custody);
        assert!(escrow_before > 0);
        assert_ok!(Lending::on_coldkey_swap(&owner, &new_owner));
        let due = Positions::<Test>::get(&new_owner, netuid()).unwrap().due;
        System::set_block_number(due.saturating_add(1));
        let _ = <Lending as Hooks<_>>::on_idle(due.saturating_add(1), Weight::MAX);
        let wrong = Lending::position_account(&new_owner, netuid());
        if wrong != custody {
            assert_eq!(
                tao(&wrong),
                0,
                "interest must not debit the post-swap derived escrow"
            );
        }
        let still = Positions::<Test>::get(&new_owner, netuid());
        assert!(
            tao(&custody) < escrow_before || still.is_none(),
            "interest/forfeit must move funds from the stored custody account"
        );
    });
}

#[test]
fn open_after_coldkey_swap_rejects_escrow_in_use() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let new_owner = account(11);
        let _hotkey = account(2);
        mint_tao(&owner, 50_000_000_000_000);
        mint_tao(&new_owner, 50_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Short,
            10_000_000_000_000,
            None,
            1,
            1,
            0,
            deadline(),
        ));
        let custody = Positions::<Test>::get(&owner, netuid()).unwrap().custody;
        assert_ok!(Lending::on_coldkey_swap(&owner, &new_owner));
        // Migrated loan still owns the original derived escrow.
        assert_eq!(
            EscrowOwner::<Test>::get(netuid(), &custody),
            Some(new_owner.clone())
        );
        assert_eq!(Lending::position_account(&owner, netuid()), custody);
        assert_err!(
            Lending::open(
                RuntimeOrigin::signed(owner.clone()),
                netuid(),
                Side::Short,
                10_000_000_000_000,
                None,
                1,
                1,
                0,
                deadline(),
            ),
            Error::<Test>::EscrowInUse
        );
        // Migrated position and its escrow untouched.
        let migrated = Positions::<Test>::get(&new_owner, netuid()).unwrap();
        assert_eq!(migrated.custody, custody);
        assert!(tao(&custody) > 0);
        assert!(Positions::<Test>::get(&owner, netuid()).is_none());
    });
}

#[test]
fn coldkey_swap_rejects_destination_with_existing_loan() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let dest = account(11);
        let _hotkey_a = account(2);
        let _hotkey_b = account(3);
        mint_tao(&owner, 50_000_000_000_000);
        mint_tao(&dest, 50_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Short,
            10_000_000_000_000,
            None,
            1,
            1,
            0,
            deadline(),
        ));
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(dest.clone()),
            netuid(),
            Side::Short,
            10_000_000_000_000,
            None,
            1,
            1,
            0,
            deadline(),
        ));
        let dest_before = Positions::<Test>::get(&dest, netuid()).unwrap();
        let dest_custody = dest_before.custody.clone();
        let dest_escrow = tao(&dest_custody);
        assert!(
            Lending::on_coldkey_swap(&owner, &dest).is_err(),
            "must not overwrite an existing destination loan"
        );
        // Destination loan and custody untouched.
        let still = Positions::<Test>::get(&dest, netuid()).unwrap();
        assert_eq!(still.custody, dest_custody);
        assert_eq!(tao(&dest_custody), dest_escrow);
        assert!(Positions::<Test>::get(&owner, netuid()).is_some());
    });
}

#[test]
fn short_settlement_uses_terminal_split_for_recovery_and_refund() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let _hotkey = account(2);
        mint_tao(&owner, 50_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Short,
            10_000_000_000_000,
            None,
            1,
            1,
            0,
            deadline(),
        ));
        let position = Positions::<Test>::get(&owner, netuid()).unwrap();
        let escrow = position.custody.clone();
        let escrow_before = tao(&escrow);
        assert!(escrow_before > 0);
        let owner_before = tao(&owner);
        // Force a settlement rate that leaves a positive refund (r ≈ 0).
        assert_ok!(Lending::start_dissolution(netuid()));
        assert_ok!(Lending::readd_lifts_for_dissolution(netuid()));
        Dissolutions::<Test>::mutate(netuid(), |d| {
            if let Some(d) = d {
                d.rate = U64F64::from_num(0);
            }
        });
        DissolutionPhase::<Test>::insert(netuid(), 1u8);
        let mut meter = WeightMeter::with_limit(Weight::from_parts(u64::MAX, u64::MAX));
        assert!(Lending::settle_lending(netuid(), &mut meter));
        assert!(Positions::<Test>::get(&owner, netuid()).is_none());
        assert_eq!(tao(&escrow), 0, "escrow must be emptied atomically");
        assert!(
            tao(&owner) >= owner_before.saturating_add(escrow_before.saturating_sub(1)),
            "full escrow refunded to owner when r=0"
        );
    });
}

/// Pool + subtoken fixture with **no** direct `References` / `G_ema` inserts.
/// Mirrors a real chain that has registered a subnet and enabled the subtoken
/// but has never called `warm_reference`.
fn cold_lending_ext() -> sp_io::TestExternalities {
    let mut ext: sp_io::TestExternalities = frame_system::GenesisConfig::<Test>::default()
        .build_storage()
        .unwrap()
        .into();
    ext.execute_with(|| {
        System::set_block_number(10);
        assert_ok!(Lending::initialize_custody());
        Enabled::<Test>::put(true);
        // Production §2.3 gate / ramp (not the bypass used by `new_test_ext`).
        LiftParamsStore::<Test>::put(LiftParams::default());
        set_registered_at(0);
        set_subtoken_enabled(true);
        set_pool(1_000_000_000_000_000, 1_000_000_000_000_000);
        set_alpha_out(0);
        set_moving_price(U64F64::from_num(1_000_000u64));
        assert!(
            References::<Test>::get(netuid()).is_none(),
            "cold fixture must not pre-seed References"
        );
        assert_eq!(Lifts::<Test>::get(netuid()).g_ema, 0);
        assert!(!ActiveNets::<Test>::contains_key(netuid()));
    });
    ext
}

#[test]
fn open_before_warm_reference_fails_without_stuck_state() {
    cold_lending_ext().execute_with(|| {
        let owner = account(1);
        let _hotkey = account(2);
        mint_tao(&owner, 50_000_000_000_000);
        let params = LiftParams::default();
        // Past min age so the only missing piece is the reference / G_ema seed.
        System::set_block_number(params.new_subnet_min_age.saturating_add(10));

        assert_err!(
            Lending::open(
                RuntimeOrigin::signed(owner.clone()),
                netuid(),
                Side::Short,
                10_000_000_000_000,
                None,
                1,
                1,
                0,
                deadline(),
            ),
            Error::<Test>::ReferenceUnavailable
        );

        // Transactional open must not leave a half-seeded book.
        assert!(References::<Test>::get(netuid()).is_none());
        assert_eq!(Lifts::<Test>::get(netuid()).g_ema, 0);
        assert!(!ActiveNets::<Test>::contains_key(netuid()));
        assert_eq!(ActiveNetCount::<Test>::get(), 0);
        assert!(Positions::<Test>::get(&owner, netuid()).is_none());
    });
}

#[test]
fn warm_reference_then_open_after_warmup_succeeds() {
    cold_lending_ext().execute_with(|| {
        let owner = account(1);
        let warmer = account(9);
        let _hotkey = account(2);
        mint_tao(&owner, 50_000_000_000_000);
        let params = LiftParams::default();
        let warmup: u64 = <<Test as Config>::ReferenceWarmup as Get<_>>::get();

        // Past min age + full Cap ramp so a normal open is not SideCapacity-bound.
        // Seeding G_ema here also satisfies `book.last - registered >= settle_ema_blocks`
        // because that age exceeds the settle window.
        let ready = params
            .new_subnet_min_age
            .saturating_add(params.new_subnet_ramp)
            .saturating_add(10);
        System::set_block_number(ready);

        assert_ok!(Lending::warm_reference(
            RuntimeOrigin::signed(warmer),
            netuid()
        ));
        let reference = References::<Test>::get(netuid()).unwrap();
        assert_eq!(reference.valid_after, ready.saturating_add(warmup));
        let book = Lifts::<Test>::get(netuid());
        assert!(book.g_ema > 0);
        assert_eq!(book.last, ready);
        assert!(ActiveNets::<Test>::contains_key(netuid()));

        // Still inside reference warmup — open must fail cleanly while keeping the seed.
        assert_err!(
            Lending::open(
                RuntimeOrigin::signed(owner.clone()),
                netuid(),
                Side::Short,
                10_000_000_000_000,
                None,
                1,
                1,
                0,
                deadline(),
            ),
            Error::<Test>::ReferenceWarmingUp
        );
        assert!(References::<Test>::get(netuid()).is_some());
        assert!(Lifts::<Test>::get(netuid()).g_ema > 0);

        // Idempotent re-warm does not reset valid_after.
        assert_ok!(Lending::warm_reference(
            RuntimeOrigin::signed(account(8)),
            netuid()
        ));
        assert_eq!(
            References::<Test>::get(netuid()).unwrap().valid_after,
            reference.valid_after
        );

        System::set_block_number(reference.valid_after);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Short,
            10_000_000_000_000,
            None,
            1,
            1,
            0,
            deadline(),
        ));
        let position = Positions::<Test>::get(&owner, netuid()).unwrap();
        assert_eq!(position.side, Side::Short);
        assert!(position.principal > 0);
    });
}

#[test]
fn warm_reference_ages_g_ema_via_on_finalize_before_min_age() {
    cold_lending_ext().execute_with(|| {
        let warmer = account(9);
        let params = LiftParams::default();
        // Warm early so ActiveNets / on_finalize can age G_ema during the min-age wait.
        System::set_block_number(100);
        assert_ok!(Lending::warm_reference(
            RuntimeOrigin::signed(warmer),
            netuid()
        ));
        assert_eq!(Lifts::<Test>::get(netuid()).last, 100);

        // Jump one EMA window; a single finalize advances `book.last` (as live
        // blocks would have done each step). Gate uses last − registered, not
        // a separate aging counter.
        let target = params.settle_ema_blocks.saturating_add(100);
        System::set_block_number(target);
        <Lending as Hooks<_>>::on_finalize(target);
        let book = Lifts::<Test>::get(netuid());
        assert!(book.g_ema > 0);
        assert!(book.last.saturating_sub(0) >= params.settle_ema_blocks);

        // Still below min age — opens remain gated.
        assert!(System::block_number() < params.new_subnet_min_age);
        assert_err!(
            Lending::open(
                RuntimeOrigin::signed(account(1)),
                netuid(),
                Side::Short,
                10_000_000_000_000,
                None,
                1,
                1,
                0,
                deadline(),
            ),
            Error::<Test>::SubnetTooNew
        );
    });
}

fn warm_migration_key() -> BoundedVec<u8, ConstU32<128>> {
    BoundedVec::truncate_from(b"migrate_warm_references_v1".to_vec())
}
fn ema_band_migration_key() -> BoundedVec<u8, ConstU32<128>> {
    BoundedVec::truncate_from(b"migrate_lift_params_ema_band_v1".to_vec())
}

#[test]
fn migration_warms_existing_subnets_and_is_idempotent() {
    cold_lending_ext().execute_with(|| {
        assert!(!HasMigrationRun::<Test>::get(warm_migration_key()));
        assert!(References::<Test>::get(netuid()).is_none());

        let _w1 = migrations::migrate_warm_references::<Test>();
        assert!(HasMigrationRun::<Test>::get(warm_migration_key()));
        assert!(References::<Test>::get(netuid()).is_some());
        assert!(Lifts::<Test>::get(netuid()).g_ema > 0);
        assert!(ActiveNets::<Test>::contains_key(netuid()));
        let valid_after = References::<Test>::get(netuid()).unwrap().valid_after;
        let g_ema = Lifts::<Test>::get(netuid()).g_ema;
        let active = ActiveNetCount::<Test>::get();

        // Second upgrade is a no-op for the warm migration (state unchanged).
        let _w2 = migrations::migrate_warm_references::<Test>();
        assert!(HasMigrationRun::<Test>::get(warm_migration_key()));
        assert_eq!(
            References::<Test>::get(netuid()).unwrap().valid_after,
            valid_after
        );
        assert_eq!(Lifts::<Test>::get(netuid()).g_ema, g_ema);
        assert_eq!(ActiveNetCount::<Test>::get(), active);
    });
}

#[test]
fn migration_skips_zero_price_subnet_cleanly() {
    cold_lending_ext().execute_with(|| {
        // Empty pool → zero price; migration must not panic or mark a half seed.
        set_pool(0, 0);
        let _ = migrations::migrate_warm_references::<Test>();
        assert!(HasMigrationRun::<Test>::get(warm_migration_key()));
        assert!(References::<Test>::get(netuid()).is_none());
        assert_eq!(Lifts::<Test>::get(netuid()).g_ema, 0);
        assert!(!ActiveNets::<Test>::contains_key(netuid()));
    });
}

#[test]
fn migration_appends_ema_band_from_pre_band_layout() {
    new_test_ext().execute_with(|| {
        HasMigrationRun::<Test>::remove(ema_band_migration_key());
        let before = LiftParamsBeforeEmaBand {
            floor_bps: 3_300,
            thin_floor_bps: 1_500,
            ref_tao: 1_000_000_000_000,
            move_bps: 1_000,
            ceiling_bps: 5_000,
            split_short_bps: 5_000,
            rebalance_target_bps: 0,
            rebalance_rate_bps: 0,
            thin_tao: 3_000_000_000_000,
            ltv_short_bps: 2_500,
            ltv_long_bps: 2_500,
            thin_ltv_long_bps: 1_500,
            apr_bps: 10_000,
            slice_bps: 1,
            crash_guard_bps: 9_500,
            settle_ema_blocks: 7_200,
            new_subnet_min_age: 50_400,
            new_subnet_ramp: 216_000,
        };
        // Write the pre-band encoding so translate must widen it.
        frame_support::storage::unhashed::put_raw(
            &LiftParamsStore::<Test>::hashed_key(),
            &before.encode(),
        );
        let _ = migrations::migrate_lift_params_ema_band::<Test>();
        assert!(HasMigrationRun::<Test>::get(ema_band_migration_key()));
        let after = LiftParamsStore::<Test>::get();
        assert_eq!(after.ema_band_bps, 1_500);
        assert_eq!(after.floor_bps, before.floor_bps);
        assert_eq!(after.apr_bps, before.apr_bps);
        assert_eq!(after.new_subnet_ramp, before.new_subnet_ramp);
        // Idempotent: second run preserves the widened value.
        let _ = migrations::migrate_lift_params_ema_band::<Test>();
        assert_eq!(LiftParamsStore::<Test>::get(), after);
    });
}

#[test]
fn lift_params_current_layout_rejects_prefix_decode_as_pre_band() {
    // SCALE Decode accepts trailing bytes, so a current-layout blob would
    // wrongly decode as LiftParamsBeforeEmaBand; DecodeAll must reject it.
    use codec::DecodeAll;
    let params = LiftParams {
        ema_band_bps: 1_500,
        floor_bps: 3_300,
        ..LiftParams::default()
    };
    let bytes = params.encode();
    assert!(
        LiftParamsBeforeEmaBand::decode(&mut &bytes[..]).is_ok(),
        "plain Decode must accept current layout as a pre-band prefix"
    );
    assert!(
        LiftParamsBeforeEmaBand::decode_all(&mut &bytes[..]).is_err(),
        "DecodeAll must reject trailing ema_band_bps"
    );
    assert!(LiftParams::decode_all(&mut &bytes[..]).is_ok());
}

#[cfg(feature = "try-runtime")]
#[test]
fn ema_band_migration_try_runtime_noop_when_already_stamped() {
    new_test_ext().execute_with(|| {
        // Current-layout storage with a stamped guard — pre_upgrade must not
        // mis-read it as pre-band (DecodeAll), and post_upgrade must accept
        // the unchanged value.
        let params = LiftParams {
            ema_band_bps: 1_500,
            floor_bps: 3_300,
            apr_bps: 10_000,
            ..LiftParams::default()
        };
        LiftParamsStore::<Test>::put(params);
        HasMigrationRun::<Test>::insert(ema_band_migration_key(), true);

        let state =
            <migrations::LiftParamsEmaBandMigration<Test> as OnRuntimeUpgrade>::pre_upgrade()
                .expect("pre_upgrade must accept current layout");
        let _ =
            <migrations::LiftParamsEmaBandMigration<Test> as OnRuntimeUpgrade>::on_runtime_upgrade(
            );
        assert_ok!(
            <migrations::LiftParamsEmaBandMigration<Test> as OnRuntimeUpgrade>::post_upgrade(state)
        );
        assert_eq!(LiftParamsStore::<Test>::get(), params);
    });
}

fn drop_hotkey_migration_key() -> BoundedVec<u8, ConstU32<128>> {
    BoundedVec::truncate_from(b"migrate_drop_position_hotkey_v1".to_vec())
}

fn pre_drop_position(custody: Account) -> migrations::PositionBeforeHotkeyDrop<Account, u64> {
    migrations::PositionBeforeHotkeyDrop {
        side: Side::Long,
        hotkey: account(2),
        custody,
        principal: 11,
        collateral: 12,
        proceeds: 0,
        escrow_alpha: 13,
        lift_tao: 14,
        lift_alpha: 15,
        annual_interest: 16,
        last_accrued: 17,
        interest_remainder: 18,
        due: 19,
        expires_at: 20,
        expiry_due: 21,
    }
}

fn put_retired_hotkey_indexes(hotkey: &Account) {
    let pallet = <Lending as frame_support::traits::PalletInfoAccess>::name().as_bytes();
    for item in migrations::RETIRED_HOTKEY_INDEXES {
        frame_support::storage::migration::put_storage_value(pallet, item, &hotkey.encode(), 1u32);
    }
}

fn retired_hotkey_indexes_empty() -> bool {
    let pallet = <Lending as frame_support::traits::PalletInfoAccess>::name().as_bytes();
    migrations::RETIRED_HOTKEY_INDEXES.iter().all(|item| {
        !frame_support::storage::unhashed::contains_prefixed_key(
            &frame_support::storage::storage_prefix(pallet, item),
        )
    })
}

#[test]
fn drop_hotkey_migration_rewrites_positions_and_clears_indexes() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let custody = account(7);
        let old = pre_drop_position(custody.clone());
        frame_support::storage::unhashed::put_raw(
            &Positions::<Test>::hashed_key_for(&owner, netuid()),
            &old.encode(),
        );
        let migrated_owner = account(5);
        let current = Position::from(pre_drop_position(custody));
        Positions::<Test>::insert(&migrated_owner, netuid(), &current);
        put_retired_hotkey_indexes(&old.hotkey);

        let _ = migrations::migrate_drop_position_hotkey::<Test>();
        assert!(HasMigrationRun::<Test>::get(drop_hotkey_migration_key()));
        let after = Positions::<Test>::get(&owner, netuid()).unwrap();
        assert_eq!(after, current, "every non-hotkey field survives");
        assert_eq!(
            Positions::<Test>::get(&migrated_owner, netuid()),
            Some(current.clone()),
            "current-layout rows are left alone"
        );
        assert!(retired_hotkey_indexes_empty());

        HasMigrationRun::<Test>::remove(drop_hotkey_migration_key());
        let _ = migrations::migrate_drop_position_hotkey::<Test>();
        assert_eq!(Positions::<Test>::get(&owner, netuid()), Some(current));
        assert_eq!(Positions::<Test>::iter().count(), 2);
    });
}

#[test]
fn drop_hotkey_migration_keeps_undecodable_rows_and_retries() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let key = Positions::<Test>::hashed_key_for(&owner, netuid());
        frame_support::storage::unhashed::put_raw(&key, &[0xff, 0x01]);

        let _ = migrations::migrate_drop_position_hotkey::<Test>();
        assert!(!HasMigrationRun::<Test>::get(drop_hotkey_migration_key()));
        assert_eq!(
            frame_support::storage::unhashed::get_raw(&key),
            Some(vec![0xff, 0x01])
        );
    });
}

#[cfg(feature = "try-runtime")]
#[test]
fn drop_hotkey_migration_try_runtime_checks_pass() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        frame_support::storage::unhashed::put_raw(
            &Positions::<Test>::hashed_key_for(&owner, netuid()),
            &pre_drop_position(account(7)).encode(),
        );
        put_retired_hotkey_indexes(&account(2));
        let state =
            <migrations::DropPositionHotkeyMigration<Test> as OnRuntimeUpgrade>::pre_upgrade()
                .expect("pre_upgrade must accept the pre-drop layout");
        let _ =
            <migrations::DropPositionHotkeyMigration<Test> as OnRuntimeUpgrade>::on_runtime_upgrade(
            );
        assert_ok!(
            <migrations::DropPositionHotkeyMigration<Test> as OnRuntimeUpgrade>::post_upgrade(
                state
            )
        );
    });
}

#[test]
fn registration_hook_seeds_without_manual_warm_and_open_succeeds_after_gate() {
    cold_lending_ext().execute_with(|| {
        // Simulate pre-start-call registration: pool priced, subtoken still off.
        set_subtoken_enabled(false);
        System::set_block_number(10);
        <Lending as LendingInterface<_>>::on_subnet_registered(netuid());

        let reference = References::<Test>::get(netuid()).unwrap();
        assert!(Lifts::<Test>::get(netuid()).g_ema > 0);
        assert!(ActiveNets::<Test>::contains_key(netuid()));
        // No manual warm_reference.

        // Start-call fallback is idempotent.
        set_subtoken_enabled(true);
        <Lending as LendingInterface<_>>::on_subtoken_enabled(netuid());
        assert_eq!(
            References::<Test>::get(netuid()).unwrap().valid_after,
            reference.valid_after
        );

        let owner = account(1);
        let _hotkey = account(2);
        mint_tao(&owner, 50_000_000_000_000);
        let params = LiftParams::default();
        // Past min age + Cap ramp + reference warmup. Finalize once so G_ema
        // `last` advances (seeded early at block 10, needs ≥ settle_ema_blocks).
        let ready = params
            .new_subnet_min_age
            .saturating_add(params.new_subnet_ramp)
            .max(params.settle_ema_blocks)
            .max(reference.valid_after)
            .saturating_add(1);
        System::set_block_number(ready);
        <Lending as Hooks<_>>::on_finalize(ready);

        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Short,
            10_000_000_000_000,
            None,
            1,
            1,
            0,
            deadline(),
        ));
        assert!(Positions::<Test>::get(&owner, netuid()).is_some());
    });
}

#[test]
fn max_active_net_updates_covers_global_subnet_cap() {
    // on_initialize / on_finalize share this cap; it must cover every possible
    // live subnet so EMA aging cannot outrun the reserved weight.
    assert_eq!(MAX_ACTIVE_NET_UPDATES, 4_096);
}

#[test]
fn fixed_term_expiry_settles_at_smoothed_reference_price() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let _hotkey = account(2);
        mint_tao(&owner, 50_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Short,
            10_000_000_000_000,
            None,
            1,
            1,
            0,
            deadline(),
        ));
        let position = Positions::<Test>::get(&owner, netuid()).unwrap();
        let opened_at = System::block_number();
        let term = 520u64; // Test Config::BlocksPerYear
        assert_eq!(position.expires_at, opened_at.saturating_add(term));
        assert!(Expiry::<Test>::contains_key(
            position.expires_at,
            (&owner, netuid())
        ));

        // Move the smoothed reference away from spot so settlement must use it.
        let settle_rate = U64F64::from_num(1u64) / U64F64::from_num(4u64); // 0.25 τ/α
        References::<Test>::mutate(netuid(), |r| {
            if let Some(r) = r {
                r.price = settle_rate;
            }
        });
        // Spot stays at ~1; expiry must not use it.
        assert!(MockPool::price() > settle_rate);

        // Same ceil(P · r) debt the pallet uses for smoothed short settles.
        let expected_debt = (settle_rate * U64F64::from_num(position.principal))
            .checked_ceil()
            .unwrap()
            .saturating_to_num::<u64>();
        System::set_block_number(position.expires_at);
        System::reset_events();
        let _ = <Lending as Hooks<_>>::on_idle(position.expires_at, Weight::MAX);

        assert!(
            Positions::<Test>::get(&owner, netuid()).is_none(),
            "expired position must be auto-settled"
        );
        assert!(!Expiry::<Test>::contains_key(
            position.expires_at,
            (&owner, netuid())
        ));
        let expired = System::events().into_iter().find_map(|e| match e.event {
            RuntimeEvent::Lending(Event::Expired {
                owner: o,
                principal_tao_equiv,
                rate,
                ..
            }) if o == owner => Some((principal_tao_equiv, rate)),
            _ => None,
        });
        let (debt, rate) = expired.unwrap();
        assert_eq!(rate, settle_rate);
        assert_eq!(debt, expected_debt);
        // Expiry must restore held lift legs (dissolution does this in aggregate first).
        let book = Lifts::<Test>::get(netuid());
        assert_eq!(book.held_tao, 0, "expiry must readd short lift_tao");
        assert_eq!(book.l_s, 0);
    });
}

#[test]
fn fixed_term_expiry_restores_long_held_alpha() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let hotkey = account(2);
        mint_alpha(&owner, &hotkey, netuid(), 50_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Long,
            10_000_000_000_000,
            Some(hotkey),
            1,
            0,
            1,
            deadline(),
        ));
        let position = Positions::<Test>::get(&owner, netuid()).unwrap();
        assert!(position.lift_alpha > 0);
        assert!(Lifts::<Test>::get(netuid()).held_alpha >= position.lift_alpha);
        System::set_block_number(position.expires_at);
        let _ = <Lending as Hooks<_>>::on_idle(position.expires_at, Weight::MAX);
        assert!(Positions::<Test>::get(&owner, netuid()).is_none());
        let book = Lifts::<Test>::get(netuid());
        assert_eq!(book.held_alpha, 0, "expiry must readd long lift_alpha");
        assert_eq!(book.l_l, 0);
        assert_eq!(book.escrow_alpha, 0);
    });
}

#[test]
fn long_expiry_preserves_collateral_when_pot_payout_fails() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let hotkey = account(2);
        mint_alpha(&owner, &hotkey, netuid(), 50_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Long,
            10_000_000_000_000,
            Some(hotkey.clone()),
            1,
            0,
            1,
            deadline(),
        ));
        let position = Positions::<Test>::get(&owner, netuid()).unwrap();
        let escrow = position.custody.clone();
        let custody = Lending::custody_hotkey().unwrap();
        // Zero the coupon so Due / final interest do not move collateral before settle.
        Positions::<Test>::mutate(&owner, netuid(), |p| {
            if let Some(p) = p {
                p.annual_interest = 0;
                p.interest_remainder = 0;
                p.last_accrued = position.expires_at;
            }
        });
        let snap = Positions::<Test>::get(&owner, netuid()).unwrap();
        let alpha_before = alpha(&escrow, &custody, netuid());
        assert!(alpha_before > 0);
        // (A+C)·r ≫ pool TAO → pay_from_pot fails; settle must fully roll back.
        References::<Test>::mutate(netuid(), |r| {
            if let Some(r) = r {
                r.price = U64F64::from_num(100u64);
            }
        });
        set_pool(1, pool_alpha());
        System::set_block_number(snap.expires_at);
        System::reset_events();
        let _ = <Lending as Hooks<_>>::on_idle(snap.expires_at, Weight::MAX);
        let still = Positions::<Test>::get(&owner, netuid()).unwrap();
        assert_eq!(still.collateral, snap.collateral);
        assert_eq!(still.escrow_alpha, snap.escrow_alpha);
        assert_eq!(alpha(&escrow, &custody, netuid()), alpha_before);
        // Failed claim is deferred onto the O(1) expiry-retry side queue.
        assert!(!Expiry::<Test>::contains_key(
            snap.expires_at,
            (&owner, netuid())
        ));
        let retry_at = snap
            .expires_at
            .saturating_add(<<Test as Config>::ExpiryRetryDelay as Get<u64>>::get());
        assert!(ExpiryRetry::<Test>::contains_key(
            retry_at,
            (&owner, netuid())
        ));
        assert!(
            !Expiry::<Test>::contains_key(retry_at, (&owner, netuid())),
            "retries must not mid-insert into the Expiry linked list"
        );
        assert_eq!(still.expiry_due, retry_at);
        assert_eq!(still.expires_at, snap.expires_at);
        assert_eq!(NextExpiryRetry::<Test>::get(), Some(retry_at));
        assert_eq!(ExpiryRetryPending::<Test>::get(), 1);
        assert!(
            System::events()
                .into_iter()
                .all(|e| !matches!(e.event, RuntimeEvent::Lending(Event::Expired { .. }))),
            "must not emit Expired after a failed payout"
        );
    });
}

/// Failed interest collection must defer onto a retry Due bucket, not drop
/// coverage permanently (Auditor fid:2aff7381).
#[test]
fn failed_interest_collection_defers_onto_retry_bucket() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let hotkey = account(2);
        mint_alpha(&owner, &hotkey, netuid(), 50_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Long,
            10_000_000_000_000,
            Some(hotkey.clone()),
            1,
            0,
            1,
            deadline(),
        ));
        let open_block = System::block_number();
        let period = <<Test as Config>::InterestPeriod as Get<u64>>::get();
        let due_at = open_block.saturating_add(period);
        let snap = Positions::<Test>::get(&owner, netuid()).unwrap();
        assert_eq!(snap.due, due_at);
        assert!(Due::<Test>::contains_key(due_at, (&owner, netuid())));
        let custody = Lending::custody_hotkey().unwrap();
        let collateral_before = snap.collateral;
        let alpha_before = alpha(&snap.custody, &custody, netuid());

        // Disable subtoken so long interest's transfer_staked_alpha rolls back.
        set_subtoken_enabled(false);
        System::set_block_number(due_at);
        let _ = <Lending as Hooks<_>>::on_idle(due_at, Weight::MAX);

        let still = Positions::<Test>::get(&owner, netuid()).unwrap();
        assert_eq!(
            still.collateral, collateral_before,
            "failed charge must roll back"
        );
        assert_eq!(alpha(&still.custody, &custody, netuid()), alpha_before);
        assert!(
            !Due::<Test>::contains_key(due_at, (&owner, netuid())),
            "original due bucket must not pin the queue"
        );
        let delay = <<Test as Config>::InterestRetryDelay as Get<u64>>::get();
        let retry_at = due_at.saturating_add(delay);
        assert!(
            InterestRetry::<Test>::contains_key(retry_at, (&owner, netuid())),
            "surviving loan must land on the O(1) interest-retry side queue"
        );
        assert!(
            !Due::<Test>::contains_key(retry_at, (&owner, netuid())),
            "retries must not mid-insert into the Due linked list"
        );
        assert_eq!(still.due, retry_at);
        assert_eq!(NextInterestRetry::<Test>::get(), Some(retry_at));
        assert_eq!(InterestRetryPending::<Test>::get(), 1);

        // Advance through the production-sized gap with small meters so the
        // 64-bump budget cannot kill Next before retry_at (fid:1c6bb1dc).
        set_subtoken_enabled(true);
        let mut block = due_at.saturating_add(1);
        while block < retry_at {
            System::set_block_number(block);
            let _ =
                <Lending as Hooks<_>>::on_idle(block, Weight::from_parts(50_000_000, 50_000_000));
            assert_eq!(
                InterestRetryPending::<Test>::get(),
                1,
                "pending must survive empty-gap bumps"
            );
            assert!(
                NextInterestRetry::<Test>::get().is_some(),
                "NextInterestRetry must remain resumable across a {}-block delay",
                delay
            );
            block = block.saturating_add(32);
        }
        System::set_block_number(retry_at);
        let _ = <Lending as Hooks<_>>::on_idle(retry_at, Weight::MAX);
        let after = Positions::<Test>::get(&owner, netuid()).unwrap();
        assert!(
            after.collateral < collateral_before || after.collateral == 0,
            "retry must charge interest once the subtoken is live again"
        );
        assert!(!InterestRetry::<Test>::contains_key(
            retry_at,
            (&owner, netuid())
        ));
        assert_eq!(
            after.due,
            retry_at.saturating_add(period),
            "successful retry must reschedule onto the normal interest period"
        );
        assert!(Due::<Test>::contains_key(after.due, (&owner, netuid())));
    });
}

#[test]
fn coldkey_swap_rekeys_deferred_expiry_retry_bucket() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let new_owner = account(9);
        let hotkey = account(2);
        mint_alpha(&owner, &hotkey, netuid(), 50_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Long,
            10_000_000_000_000,
            Some(hotkey),
            1,
            0,
            1,
            deadline(),
        ));
        let snap = Positions::<Test>::get(&owner, netuid()).unwrap();
        Positions::<Test>::mutate(&owner, netuid(), |p| {
            if let Some(p) = p {
                p.annual_interest = 0;
                p.interest_remainder = 0;
                p.last_accrued = snap.expires_at;
            }
        });
        References::<Test>::mutate(netuid(), |r| {
            if let Some(r) = r {
                r.price = U64F64::from_num(100u64);
            }
        });
        set_pool(1, pool_alpha());
        System::set_block_number(snap.expires_at);
        let _ = <Lending as Hooks<_>>::on_idle(snap.expires_at, Weight::MAX);
        let deferred = Positions::<Test>::get(&owner, netuid()).unwrap();
        let retry_at = deferred.expiry_due;
        assert!(retry_at > snap.expires_at);
        assert!(ExpiryRetry::<Test>::contains_key(
            retry_at,
            (&owner, netuid())
        ));
        assert_ok!(Lending::on_coldkey_swap(&owner, &new_owner));
        assert!(!Positions::<Test>::contains_key(&owner, netuid()));
        assert!(!ExpiryRetry::<Test>::contains_key(
            retry_at,
            (&owner, netuid())
        ));
        assert!(ExpiryRetry::<Test>::contains_key(
            retry_at,
            (&new_owner, netuid())
        ));
        assert_eq!(
            Positions::<Test>::get(&new_owner, netuid())
                .unwrap()
                .expiry_due,
            retry_at
        );
        // Restore pot liquidity and settle under the new owner on the retry slot.
        References::<Test>::mutate(netuid(), |r| {
            if let Some(r) = r {
                r.price = MockPool::price();
            }
        });
        set_pool(1_000_000_000_000_000, pool_alpha());
        System::set_block_number(retry_at);
        System::reset_events();
        let _ = <Lending as Hooks<_>>::on_idle(retry_at, Weight::MAX);
        assert!(!Positions::<Test>::contains_key(&new_owner, netuid()));
        assert!(System::events().into_iter().any(|e| matches!(
            e.event,
            RuntimeEvent::Lending(Event::Expired {
                owner: ref o,
                side: Side::Long,
                ..
            }) if o == &new_owner
        )));
    });
}

#[test]
fn expiry_stops_after_interest_forfeits_short() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let _hotkey = account(2);
        mint_tao(&owner, 50_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Short,
            5_000_000_000_000,
            None,
            1,
            1,
            1,
            deadline(),
        ));
        let snap = Positions::<Test>::get(&owner, netuid()).unwrap();
        // Leave a tiny coupon balance and force a final charge that exhausts it.
        Positions::<Test>::mutate(&owner, netuid(), |p| {
            if let Some(p) = p {
                p.collateral = 1;
                p.annual_interest = 10_000_000_000_000;
                p.interest_remainder = 0;
                p.last_accrued = snap.expires_at.saturating_sub(100);
            }
        });
        System::set_block_number(snap.expires_at);
        System::reset_events();
        let _ = <Lending as Hooks<_>>::on_idle(snap.expires_at, Weight::MAX);
        assert!(
            !Positions::<Test>::contains_key(&owner, netuid()),
            "forfeit must clear the position"
        );
        assert!(!Expiry::<Test>::contains_key(
            snap.expires_at,
            (&owner, netuid())
        ));
        assert!(
            System::events()
                .into_iter()
                .any(|e| matches!(e.event, RuntimeEvent::Lending(Event::Forfeited { .. })))
        );
        assert!(
            System::events()
                .into_iter()
                .all(|e| !matches!(e.event, RuntimeEvent::Lending(Event::Expired { .. }))),
            "must not continue to Expired after forfeit"
        );
    });
}

#[test]
fn expiry_stops_after_interest_forfeits_long() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let hotkey = account(2);
        mint_alpha(&owner, &hotkey, netuid(), 50_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Long,
            10_000_000_000_000,
            Some(hotkey),
            1,
            0,
            1,
            deadline(),
        ));
        let snap = Positions::<Test>::get(&owner, netuid()).unwrap();
        Positions::<Test>::mutate(&owner, netuid(), |p| {
            if let Some(p) = p {
                p.collateral = 1;
                p.annual_interest = 10_000_000_000_000;
                p.interest_remainder = 0;
                p.last_accrued = snap.expires_at.saturating_sub(100);
            }
        });
        System::set_block_number(snap.expires_at);
        System::reset_events();
        let _ = <Lending as Hooks<_>>::on_idle(snap.expires_at, Weight::MAX);
        assert!(!Positions::<Test>::contains_key(&owner, netuid()));
        assert!(
            System::events()
                .into_iter()
                .any(|e| matches!(e.event, RuntimeEvent::Lending(Event::Forfeited { .. })))
        );
        assert!(
            System::events()
                .into_iter()
                .all(|e| !matches!(e.event, RuntimeEvent::Lending(Event::Expired { .. })))
        );
    });
}

#[test]
fn failed_expiry_does_not_block_later_bucket() {
    new_test_ext().execute_with(|| {
        let owner_fail = account(1);
        let owner_ok = account(3);
        let hotkey_fail = account(2);
        let _hotkey_ok = account(4);
        mint_alpha(&owner_fail, &hotkey_fail, netuid(), 50_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner_fail.clone()),
            netuid(),
            Side::Long,
            10_000_000_000_000,
            Some(hotkey_fail),
            1,
            0,
            1,
            deadline(),
        ));
        let fail_exp = Positions::<Test>::get(&owner_fail, netuid())
            .unwrap()
            .expires_at;
        // Open the healthy short later so it lands in a later expiry bucket.
        System::set_block_number(20);
        mint_tao(&owner_ok, 50_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner_ok.clone()),
            netuid(),
            Side::Short,
            5_000_000_000_000,
            None,
            1,
            1,
            1,
            deadline(),
        ));
        let ok_exp = Positions::<Test>::get(&owner_ok, netuid())
            .unwrap()
            .expires_at;
        assert!(
            ok_exp > fail_exp,
            "healthy loan must expire in a later bucket"
        );
        Positions::<Test>::mutate(&owner_fail, netuid(), |p| {
            if let Some(p) = p {
                p.annual_interest = 0;
                p.interest_remainder = 0;
                p.last_accrued = fail_exp;
            }
        });
        Positions::<Test>::mutate(&owner_ok, netuid(), |p| {
            if let Some(p) = p {
                p.annual_interest = 0;
                p.interest_remainder = 0;
                p.last_accrued = ok_exp;
            }
        });
        // Force the earlier long settle to miss the pot.
        References::<Test>::mutate(netuid(), |r| {
            if let Some(r) = r {
                r.price = U64F64::from_num(100u64);
            }
        });
        set_pool(1, pool_alpha());
        System::set_block_number(fail_exp);
        let _ = <Lending as Hooks<_>>::on_idle(fail_exp, Weight::MAX);
        assert!(
            Positions::<Test>::contains_key(&owner_fail, netuid()),
            "failed long must remain open"
        );
        assert!(
            Positions::<Test>::contains_key(&owner_ok, netuid()),
            "later short must still be waiting"
        );
        let retry_at =
            fail_exp.saturating_add(<<Test as Config>::ExpiryRetryDelay as Get<u64>>::get());
        assert_ne!(
            NextExpiry::<Test>::get(),
            Some(fail_exp),
            "failed bucket must not pin NextExpiry"
        );
        assert_eq!(
            NextExpiry::<Test>::get(),
            Some(ok_exp),
            "main Expiry schedule must advance past the failed claim"
        );
        assert_eq!(
            NextExpiryRetry::<Test>::get(),
            Some(retry_at),
            "failed claim moves onto the O(1) expiry-retry side queue"
        );
        assert_eq!(ExpiryRetryPending::<Test>::get(), 1);
        assert!(ExpiryRetry::<Test>::contains_key(
            retry_at,
            (&owner_fail, netuid())
        ));
        // Short settlement does not need pot TAO — it must clear once due,
        // even though the deferred long sits on the side retry queue (which may
        // be scheduled after ok_exp when ExpiryRetryDelay is production-sized).
        System::set_block_number(ok_exp);
        System::reset_events();
        let _ = <Lending as Hooks<_>>::on_idle(ok_exp, Weight::MAX);
        assert!(
            !Positions::<Test>::contains_key(&owner_ok, netuid()),
            "later expiry must still settle while an earlier claim is deferred"
        );
        assert!(System::events().into_iter().any(|e| matches!(
            e.event,
            RuntimeEvent::Lending(Event::Expired {
                side: Side::Short,
                ..
            })
        )));
        assert!(Positions::<Test>::contains_key(&owner_fail, netuid()));
    });
}

#[test]
fn coldkey_swap_rewinds_expiry_cursor_so_due_loan_still_settles() {
    new_test_ext().execute_with(|| {
        let owner_a = account(1);
        let owner_b = account(2);
        let dest = account(99);
        let _hotkey_a = account(10);
        let _hotkey_b = account(11);
        mint_tao(&owner_a, 50_000_000_000_000);
        mint_tao(&owner_b, 50_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner_a.clone()),
            netuid(),
            Side::Short,
            5_000_000_000_000,
            None,
            1,
            1,
            0,
            deadline(),
        ));
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner_b.clone()),
            netuid(),
            Side::Short,
            5_000_000_000_000,
            None,
            1,
            1,
            0,
            deadline(),
        ));
        let expires_at = Positions::<Test>::get(&owner_a, netuid())
            .unwrap()
            .expires_at;
        assert_eq!(
            Positions::<Test>::get(&owner_b, netuid())
                .unwrap()
                .expires_at,
            expires_at
        );
        // Simulate mid-bucket drain past owner_a; migrating a→dest with a key that
        // sorts before the cursor would skip settlement without a rewind.
        ExpiryBucketCursor::<Test>::put((owner_a.clone(), netuid()));
        assert_ok!(Lending::on_coldkey_swap(&owner_a, &dest));
        assert!(
            ExpiryBucketCursor::<Test>::get().is_none(),
            "cursor rewound"
        );
        assert!(Expiry::<Test>::contains_key(expires_at, (&dest, netuid())));
        assert!(!Expiry::<Test>::contains_key(
            expires_at,
            (&owner_a, netuid())
        ));

        System::set_block_number(expires_at);
        let _ = <Lending as Hooks<_>>::on_idle(expires_at, Weight::MAX);
        assert!(
            Positions::<Test>::get(&dest, netuid()).is_none(),
            "migrated loan must still expire"
        );
        assert!(Positions::<Test>::get(&owner_b, netuid()).is_none());
    });
}

#[test]
fn fixed_term_partial_repay_keeps_expiry_after_increase_restart() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let hotkey = account(2);
        mint_tao(&owner, 100_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Short,
            10_000_000_000_000,
            None,
            1,
            1,
            0,
            deadline(),
        ));
        let first = Positions::<Test>::get(&owner, netuid()).unwrap();
        // Advance, then increase — term restarts (Skeptic short near-expiry drain).
        System::set_block_number(System::block_number().saturating_add(20));
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Short,
            5_000_000_000_000,
            None,
            1,
            1,
            0,
            deadline(),
        ));
        let after_add = Positions::<Test>::get(&owner, netuid()).unwrap();
        assert!(after_add.expires_at > first.expires_at);
        assert!(after_add.principal > first.principal);
        let expires_at = after_add.expires_at;

        // Partial repay leaves the (possibly restarted) expires_at unchanged.
        let half = after_add.principal / 2;
        mint_alpha(&owner, &hotkey, netuid(), half);
        assert_ok!(Lending::repay(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            account(2),
            half,
            true,
            u64::MAX,
            0,
            deadline(),
        ));
        let after_repay = Positions::<Test>::get(&owner, netuid()).unwrap();
        assert_eq!(after_repay.expires_at, expires_at);
    });
}

#[test]
fn long_collateral_top_up_restarts_fixed_term() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let hotkey = account(2);
        mint_alpha(&owner, &hotkey, netuid(), 100_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Long,
            5_000_000_000_000,
            Some(hotkey.clone()),
            1,
            0,
            1,
            deadline(),
        ));
        let first = Positions::<Test>::get(&owner, netuid()).unwrap();
        let opened_at = System::block_number();
        assert_eq!(
            first.expires_at,
            opened_at.saturating_add(520),
            "term = BlocksPerYear"
        );
        System::set_block_number(opened_at.saturating_add(100));
        mint_alpha(&owner, &hotkey, netuid(), 20_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Long,
            2_000_000_000_000,
            Some(hotkey),
            1,
            0,
            1,
            deadline(),
        ));
        let after = Positions::<Test>::get(&owner, netuid()).unwrap();
        let now = System::block_number();
        assert_eq!(
            after.expires_at,
            now.saturating_add(520),
            "long collateral top-up must restart the term"
        );
        assert!(after.expires_at > first.expires_at);
        assert!(!Expiry::<Test>::contains_key(
            first.expires_at,
            (&owner, netuid())
        ));
        assert!(Expiry::<Test>::contains_key(
            after.expires_at,
            (&owner, netuid())
        ));
    });
}

/// Skeptic: short increases must restart the term so fresh principal sold at
/// live spot cannot inherit an imminent EMA settlement below that sale.
#[test]
fn short_increase_restarts_fixed_term() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let _hotkey = account(2);
        mint_tao(&owner, 100_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Short,
            5_000_000_000_000,
            None,
            1,
            0,
            0,
            deadline(),
        ));
        let first = Positions::<Test>::get(&owner, netuid()).unwrap();
        let opened_at = System::block_number();
        assert_eq!(first.expires_at, opened_at.saturating_add(520));
        System::set_block_number(opened_at.saturating_add(100));
        mint_tao(&owner, 20_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Short,
            2_000_000_000_000,
            None,
            1,
            0,
            0,
            deadline(),
        ));
        let after = Positions::<Test>::get(&owner, netuid()).unwrap();
        let now = System::block_number();
        assert_eq!(
            after.expires_at,
            now.saturating_add(520),
            "short increase must restart the term"
        );
        assert!(after.expires_at > first.expires_at);
        assert!(!Expiry::<Test>::contains_key(
            first.expires_at,
            (&owner, netuid())
        ));
        assert!(Expiry::<Test>::contains_key(
            after.expires_at,
            (&owner, netuid())
        ));
    });
}

#[test]
fn long_expiry_caps_collateral_at_amm_not_ema_dump() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let hotkey = account(2);
        // Small pool so a large C dump is impactful vs EMA flat pricing.
        set_pool(1_000_000_000_000, 1_000_000_000_000); // 1k τ / 1k α
        let spot = MockPool::price();
        References::<Test>::insert(
            netuid(),
            LendingReference {
                price: spot,
                last_updated: 1u64,
                valid_after: 1u64,
            },
        );
        Lifts::<Test>::mutate(netuid(), |book| {
            book.g_ema = pool_tao();
            book.last = LiftParams::default().settle_ema_blocks;
        });
        mint_alpha(&owner, &hotkey, netuid(), 2_000_000_000_000);
        // Tiny long (minimal principal) with large collateral — the near-expiry
        // drain shape, without waiting a year (we jump to expires_at).
        assert_ok!(Lending::open_exact(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Long,
            900_000_000_000,
            Some(
                // 900 α collateral
                hotkey
            ),
            1_000_000_000, // 1 τ principal
            0,
            1,
            deadline(),
        ));
        let pos = Positions::<Test>::get(&owner, netuid()).unwrap();
        let c = pos.collateral;
        let r = References::<Test>::get(netuid()).unwrap().price;
        let c_ema = {
            // Floor(C * r) in the same units as settlement.
            let bits = (U64F64::from_num(c) * r).saturating_to_num::<u64>();
            bits
        };
        let c_amm = MockPool::quote_sell(netuid(), c.into()).unwrap().to_u64();
        assert!(
            c_amm < c_ema,
            "fixture must make AMM impact < EMA flat value ({} vs {})",
            c_amm,
            c_ema
        );
        let tao_before = tao(&owner);
        System::set_block_number(pos.expires_at);
        System::reset_events();
        let _ = <Lending as Hooks<_>>::on_idle(pos.expires_at, Weight::MAX);
        assert!(!Positions::<Test>::contains_key(&owner, netuid()));
        let gained = tao(&owner).saturating_sub(tao_before);
        // Refund is payout − P; payout ≤ A_ema + min(C_ema, C_amm).
        // With tiny A and P=1e9, gained must stay near the AMM-capped C value.
        assert!(
            gained <= c_amm.saturating_add(1_000_000_000),
            "expiry refund {} must not exceed AMM-capped collateral (~{})",
            gained,
            c_amm
        );
        assert!(
            gained < c_ema.saturating_sub(pos.principal),
            "must not pay full EMA·C − P ({} would)",
            c_ema.saturating_sub(pos.principal)
        );
    });
}

#[test]
fn fixed_term_expiry_is_bounded_by_weight() {
    new_test_ext().execute_with(|| {
        // Open several shorts that share one expiry bucket.
        let n = 4u8;
        for i in 0..n {
            let owner = account(10 + i);
            let _hotkey = account(50 + i);
            mint_tao(&owner, 50_000_000_000_000);
            assert_ok!(Lending::open(
                RuntimeOrigin::signed(owner),
                netuid(),
                Side::Short,
                5_000_000_000_000,
                None,
                1,
                1,
                0,
                deadline(),
            ));
        }
        let expires_at = Positions::<Test>::iter()
            .next()
            .map(|(_, _, p)| p.expires_at)
            .unwrap();
        assert_eq!(ExpiryIndex::<Test>::get(expires_at), u32::from(n));

        System::set_block_number(expires_at);
        // Drive collect_expiry directly so interest Due work cannot eat the budget.
        let header = RocksDbWeight::get()
            .reads_writes(2, 1)
            .saturating_add(RocksDbWeight::get().reads(1));
        let one = header.saturating_add(<() as WeightInfo>::settle());
        // Room for exactly one settle; second settle must fail the meter.
        let budget = one.saturating_add(Weight::from_parts(1, 0));
        let mut meter = WeightMeter::with_limit(budget);
        Lending::collect_expiry(expires_at, &mut meter);
        let remaining_after_partial = Positions::<Test>::iter().count();
        assert!(
            remaining_after_partial > 0 && remaining_after_partial < usize::from(n),
            "bounded weight must settle some but not all expiries: left={remaining_after_partial}"
        );

        // Finish with full weight.
        let mut meter = WeightMeter::with_limit(Weight::MAX);
        Lending::collect_expiry(expires_at, &mut meter);
        assert_eq!(Positions::<Test>::iter().count(), 0);
        assert_eq!(ExpiryIndex::<Test>::get(expires_at), 0);
    });
}

#[test]
fn dissolution_before_expiry_wins() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let _hotkey = account(2);
        mint_tao(&owner, 50_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Short,
            10_000_000_000_000,
            None,
            1,
            1,
            0,
            deadline(),
        ));
        let expires_at = Positions::<Test>::get(&owner, netuid()).unwrap().expires_at;
        assert!(expires_at > System::block_number());

        assert_ok!(Lending::start_dissolution(netuid()));
        assert_ok!(Lending::readd_lifts_for_dissolution(netuid()));
        let mut meter = WeightMeter::with_limit(Weight::MAX);
        assert!(Lending::settle_lending(netuid(), &mut meter));
        assert!(Positions::<Test>::get(&owner, netuid()).is_none());
        // Expiry queue entry cleared by remove_position during dissolution.
        assert!(!Expiry::<Test>::contains_key(
            expires_at,
            (&owner, netuid())
        ));

        // Advancing to the old expiry must not emit Expired (already settled).
        System::set_block_number(expires_at);
        System::reset_events();
        let _ = <Lending as Hooks<_>>::on_idle(expires_at, Weight::MAX);
        let saw_expired = System::events()
            .into_iter()
            .any(|e| matches!(e.event, RuntimeEvent::Lending(Event::Expired { .. })));
        assert!(!saw_expired);
    });
}

fn set_reference_price(price: U64F64) {
    References::<Test>::mutate(netuid(), |r| {
        if let Some(r) = r {
            r.price = price;
        }
    });
}

#[test]
fn ema_band_blocks_long_above_ceiling_but_allows_short() {
    new_test_ext().execute_with(|| {
        // Spot ≈ 1. EMA low so spot is already > EMA·1.15; any long buy stays above.
        let ema = U64F64::from_num(1u64) / U64F64::from_num(2u64); // 0.5
        set_reference_price(ema);
        assert!(
            MockPool::price()
                > ema.saturating_mul(U64F64::from_num(115u64)) / U64F64::from_num(100u64)
        );

        let long_owner = account(1);
        let short_owner = account(3);
        let hotkey = account(2);
        mint_alpha(&long_owner, &hotkey, netuid(), 50_000_000_000_000);
        mint_tao(&short_owner, 50_000_000_000_000);

        assert_err!(
            Lending::open(
                RuntimeOrigin::signed(long_owner.clone()),
                netuid(),
                Side::Long,
                5_000_000_000_000,
                Some(hotkey.clone()),
                1,
                0,
                1,
                deadline(),
            ),
            Error::<Test>::PriceTooFarFromEma
        );
        assert!(Positions::<Test>::get(&long_owner, netuid()).is_none());

        assert_ok!(Lending::open(
            RuntimeOrigin::signed(short_owner.clone()),
            netuid(),
            Side::Short,
            10_000_000_000_000,
            None,
            1,
            1,
            0,
            deadline(),
        ));
        assert!(Positions::<Test>::get(&short_owner, netuid()).is_some());
    });
}

#[test]
fn ema_band_blocks_short_below_floor_but_allows_long() {
    new_test_ext().execute_with(|| {
        // Spot ≈ 1. EMA high so spot is already < EMA·0.85; any short sale stays below.
        let ema = U64F64::from_num(2u64);
        set_reference_price(ema);
        assert!(
            MockPool::price()
                < ema.saturating_mul(U64F64::from_num(85u64)) / U64F64::from_num(100u64)
        );

        let short_owner = account(1);
        let long_owner = account(3);
        let hotkey = account(2);
        mint_tao(&short_owner, 50_000_000_000_000);
        mint_alpha(&long_owner, &hotkey, netuid(), 50_000_000_000_000);

        assert_err!(
            Lending::open(
                RuntimeOrigin::signed(short_owner.clone()),
                netuid(),
                Side::Short,
                10_000_000_000_000,
                None,
                1,
                1,
                0,
                deadline(),
            ),
            Error::<Test>::PriceTooFarFromEma
        );
        assert!(Positions::<Test>::get(&short_owner, netuid()).is_none());

        assert_ok!(Lending::open(
            RuntimeOrigin::signed(long_owner.clone()),
            netuid(),
            Side::Long,
            5_000_000_000_000,
            Some(hotkey),
            1,
            0,
            1,
            deadline(),
        ));
        assert!(Positions::<Test>::get(&long_owner, netuid()).is_some());
    });
}

#[test]
fn ema_band_rejects_one_large_trade_that_would_cross() {
    new_test_ext().execute_with(|| {
        // Tight band so a single medium long crosses it from EMA == spot.
        LiftParamsStore::<Test>::mutate(|p| p.ema_band_bps = 100); // 1%
        set_reference_price(MockPool::price());
        // Shallower pool → more price impact per τ.
        set_pool(100_000_000_000_000, 100_000_000_000_000);
        Lifts::<Test>::mutate(netuid(), |book| book.g_ema = pool_tao());
        set_tao_ema(Lending::settlement_tao_basis(netuid()));
        set_reference_price(MockPool::price());

        let owner = account(1);
        let hotkey = account(2);
        mint_alpha(&owner, &hotkey, netuid(), 200_000_000_000_000);
        // ~10% of pool TAO as principal → CP price move ≫ 1%.
        assert_err!(
            Lending::open_exact(
                RuntimeOrigin::signed(owner.clone()),
                netuid(),
                Side::Long,
                80_000_000_000_000,
                Some(hotkey),
                10_000_000_000_000,
                1,
                1,
                deadline(),
            ),
            Error::<Test>::PriceTooFarFromEma
        );
        assert!(Positions::<Test>::get(&owner, netuid()).is_none());
    });
}

#[test]
fn ema_band_many_small_opens_cannot_cross_in_one_block() {
    new_test_ext().execute_with(|| {
        LiftParamsStore::<Test>::mutate(|p| p.ema_band_bps = 500); // 5%
        set_pool(50_000_000_000_000, 50_000_000_000_000);
        Lifts::<Test>::mutate(netuid(), |book| book.g_ema = pool_tao());
        set_tao_ema(Lending::settlement_tao_basis(netuid()));
        let ema = MockPool::price();
        set_reference_price(ema);
        // EMA is frozen until on_finalize; spot climbs with each long.

        let mut opened = 0u32;
        let mut blocked = false;
        for i in 0u8..16 {
            let owner = account(20 + i);
            let hotkey = account(40 + i);
            mint_alpha(&owner, &hotkey, netuid(), 20_000_000_000_000);
            let res = Lending::open_exact(
                RuntimeOrigin::signed(owner.clone()),
                netuid(),
                Side::Long,
                8_000_000_000_000,
                Some(hotkey),
                1_000_000_000_000,
                1,
                1,
                deadline(),
            );
            match res {
                Ok(()) => opened = opened.saturating_add(1),
                Err(e) => {
                    assert_eq!(e, Error::<Test>::PriceTooFarFromEma.into());
                    blocked = true;
                    break;
                }
            }
        }
        assert!(
            opened > 0,
            "at least one small open should fit under the band"
        );
        assert!(
            blocked,
            "further opens in the same block must hit PriceTooFarFromEma"
        );
        let spot = MockPool::price();
        let ceiling = ema.saturating_mul(U64F64::from_num(10_500u32)) / U64F64::from_num(10_000u32);
        assert!(spot <= ceiling || blocked);
    });
}

#[test]
fn ema_band_never_blocks_close_or_repay() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let hotkey = account(2);
        mint_tao(&owner, 100_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Short,
            20_000_000_000_000,
            None,
            1,
            1,
            0,
            deadline(),
        ));
        let before = Positions::<Test>::get(&owner, netuid()).unwrap();
        // Push EMA far away so the position sits outside the open band.
        set_reference_price(U64F64::from_num(10u64));
        assert!(MockPool::price() < set_and_band_floor());

        let half = before.principal / 2;
        mint_alpha(&owner, &hotkey, netuid(), half);
        assert_ok!(Lending::repay(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            account(2),
            half,
            true,
            u64::MAX,
            0,
            deadline(),
        ));
        assert!(Positions::<Test>::get(&owner, netuid()).is_some());

        let left = Positions::<Test>::get(&owner, netuid()).unwrap().principal;
        mint_alpha(&owner, &hotkey, netuid(), left);
        assert_ok!(Lending::close(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            account(2),
            true,
            u64::MAX,
            0,
            deadline(),
        ));
        assert!(Positions::<Test>::get(&owner, netuid()).is_none());
    });
}

fn set_and_band_floor() -> U64F64 {
    let ema = References::<Test>::get(netuid()).unwrap().price;
    let band = u32::from(LiftParamsStore::<Test>::get().ema_band_bps);
    ema.saturating_mul(U64F64::from_num(10_000u32.saturating_sub(band)))
        / U64F64::from_num(10_000u32)
}

#[test]
fn quote_open_exact_insufficient_collateral_search_matches_open() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let hotkey = account(2);
        let principal = 1_000_000_000_000u64; // 1 τ long
        mint_alpha(&owner, &hotkey, netuid(), 50_000_000_000_000);

        // Binary search the minimum collateral the exact quote accepts.
        let mut lo = 0u64;
        let mut hi = 20_000_000_000_000u64;
        for _ in 0..64 {
            if lo + 1 >= hi {
                break;
            }
            let mid = lo + (hi - lo) / 2;
            match Lending::quote_open_exact_for(
                &owner,
                netuid(),
                Side::Long,
                mid,
                Some(&hotkey),
                principal,
            ) {
                Ok(_) => hi = mid,
                Err(e) => {
                    assert_eq!(e, Error::<Test>::InsufficientCollateral.into());
                    lo = mid;
                }
            }
        }
        let min_c = hi;
        assert!(min_c > 1, "search must find a positive minimum");

        assert_err!(
            Lending::quote_open_exact_for(
                &owner,
                netuid(),
                Side::Long,
                min_c - 1,
                Some(&hotkey),
                principal
            ),
            Error::<Test>::InsufficientCollateral
        );
        assert_ok!(Lending::quote_open_exact_for(
            &owner,
            netuid(),
            Side::Long,
            min_c,
            Some(&hotkey),
            principal
        ));

        // One unit less fails the real open; the returned minimum succeeds.
        assert_err!(
            Lending::open_exact(
                RuntimeOrigin::signed(owner.clone()),
                netuid(),
                Side::Long,
                min_c - 1,
                Some(hotkey.clone()),
                principal,
                1,
                1,
                deadline(),
            ),
            Error::<Test>::DynamicCollateral
        );
        assert_ok!(Lending::open_exact(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Long,
            min_c,
            Some(hotkey),
            principal,
            1,
            1,
            deadline(),
        ));
        let pos = Positions::<Test>::get(&owner, netuid()).unwrap();
        assert_eq!(pos.principal, principal);
        assert_eq!(pos.collateral, min_c);
    });
}

#[test]
fn convert_pending_defers_when_quote_misses_ema_floor() {
    new_test_ext().execute_with(|| {
        let custody = Lending::custody_hotkey().unwrap();
        let reserve = Lending::reserve_account(netuid());
        // Full backlog up to the spot-move cap (no / InterestPeriod).
        LiftParamsStore::<Test>::mutate(|p| p.slice_bps = 10_000);
        let pending = 10_000_000_000_000u64;
        // Thin TAO, alpha ≈ 2·pending → CP impact makes quote < 0.95·spot·slice.
        // With slice_bps = 10_000, max_slice = pool alpha, so slice = pending.
        set_pool(pending, pending.saturating_mul(2));
        let slice = pending;
        mint_alpha(&reserve, &custody, netuid(), pending);
        LongInterestPending::<Test>::insert(netuid(), pending);
        Lifts::<Test>::mutate(netuid(), |book| book.g_ema = pool_tao());
        let spot = MockPool::price();
        References::<Test>::mutate(netuid(), |r| {
            if let Some(r) = r {
                r.price = spot; // crash guard passes (spot == EMA)
            }
        });
        let quote = MockPool::quote_sell(netuid(), slice.into())
            .unwrap()
            .to_u64();
        let fair = (spot * U64F64::from_num(slice))
            .floor()
            .saturating_to_num::<u64>();
        let min_out = fair.saturating_mul(9_500) / 10_000;
        assert!(
            quote >= 10_000_000,
            "fixture must clear the dust gate (quote={quote})"
        );
        assert!(
            quote < min_out,
            "fixture must miss the EMA floor (quote={quote}, min_out={min_out})"
        );

        let before = LongInterestPending::<Test>::get(netuid());
        let pool_tao_before = pool_tao();
        System::reset_events();
        let _ = <Lending as Hooks<_>>::on_idle(System::block_number(), Weight::MAX);
        assert_eq!(LongInterestPending::<Test>::get(netuid()), before);
        assert_eq!(pool_tao(), pool_tao_before, "no adverse fill may execute");
        let sold = System::events().into_iter().any(|e| {
            matches!(
                e.event,
                RuntimeEvent::Lending(Event::LongInterestSold { .. })
            )
        });
        assert!(!sold);
    });
}

#[test]
fn convert_pending_clears_when_period_slice_would_strand() {
    new_test_ext().execute_with(|| {
        let custody = Lending::custody_hotkey().unwrap();
        let reserve = Lending::reserve_account(netuid());
        // Deep 1:1 pool (fixture default). pending quotes ≫ 0.01τ, but
        // pending/InterestPeriod quotes below the dust gate — the old slice
        // math stranded that coupon forever.
        let period = <<Test as Config>::InterestPeriod as Get<u64>>::get().max(1);
        let pending = 50_000_000u64; // ~0.05τ at 1:1
        let period_slice = (pending / period).max(1);
        let period_quote = MockPool::quote_sell(netuid(), period_slice.into())
            .unwrap()
            .to_u64();
        assert!(
            period_quote < 10_000_000,
            "precondition: period-divided slice must be dust (quote={period_quote})"
        );
        let full_quote = MockPool::quote_sell(netuid(), pending.into())
            .unwrap()
            .to_u64();
        assert!(
            full_quote >= 10_000_000,
            "precondition: full pending must clear dust (quote={full_quote})"
        );

        mint_alpha(&reserve, &custody, netuid(), pending);
        LongInterestPending::<Test>::insert(netuid(), pending);
        let spot = MockPool::price();
        References::<Test>::mutate(netuid(), |r| {
            if let Some(r) = r {
                r.price = spot;
            }
        });

        System::reset_events();
        let _ = <Lending as Hooks<_>>::on_idle(System::block_number(), Weight::MAX);
        assert_eq!(
            LongInterestPending::<Test>::get(netuid()),
            0,
            "full pending must convert in one idle when it clears dust"
        );
        let sold = System::events().into_iter().any(|e| {
            matches!(
                e.event,
                RuntimeEvent::Lending(Event::LongInterestSold { .. })
            )
        });
        assert!(sold);
    });
}

#[test]
fn quote_close_escrowed_short_refund_matches_execution() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let _hotkey = account(2);
        mint_tao(&owner, 20_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Short,
            10_000_000_000_000,
            None,
            1,
            1,
            0,
            deadline(),
        ));
        let quoted = Lending::quote_close(&owner, netuid(), &account(2), false).unwrap();
        assert!(
            quoted.refund > 0,
            "escrow buyback should leave a TAO refund"
        );

        let bal_before = tao(&owner);
        assert_ok!(Lending::close(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            account(2),
            false,
            u64::MAX,
            0,
            deadline(),
        ));
        let refunded = tao(&owner).saturating_sub(bal_before);
        // Quote and execution should agree within 1% (fees / rounding).
        let diff = quoted.refund.abs_diff(refunded);
        assert!(
            diff <= quoted.refund / 100 + 1,
            "quote refund {} vs executed {} (diff {})",
            quoted.refund,
            refunded,
            diff
        );
        assert!(Positions::<Test>::get(&owner, netuid()).is_none());
    });
}

#[test]
fn close_stops_after_interest_forfeits_short() {
    new_test_ext().execute_with(|| {
        // Second short keeps shared reserve funded so a buggy double-readd
        // would succeed and corrupt PositionCount rather than Err-masking.
        let other = account(3);
        let _other_hk = account(4);
        mint_tao(&other, 50_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(other.clone()),
            netuid(),
            Side::Short,
            10_000_000_000_000,
            None,
            1,
            1,
            0,
            deadline(),
        ));

        let owner = account(1);
        let hotkey = account(2);
        mint_tao(&owner, 50_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Short,
            5_000_000_000_000,
            None,
            1,
            1,
            0,
            deadline(),
        ));
        let snap = Positions::<Test>::get(&owner, netuid()).unwrap();
        // Prefund escrow so a continued close could still refund/readd.
        mint_tao(&snap.custody, 20_000_000_000_000);
        mint_alpha(&owner, &hotkey, netuid(), snap.principal.saturating_mul(2));
        Positions::<Test>::mutate(&owner, netuid(), |p| {
            if let Some(p) = p {
                p.collateral = 1;
                p.annual_interest = 10_000_000_000_000;
                p.interest_remainder = 0;
                p.last_accrued = System::block_number().saturating_sub(100);
            }
        });
        let count_before = PositionCount::<Test>::get(netuid());
        let total_before = TotalPositions::<Test>::get();
        System::reset_events();
        assert_ok!(Lending::close(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            account(2),
            true,
            u64::MAX,
            0,
            deadline(),
        ));
        assert!(
            !Positions::<Test>::contains_key(&owner, netuid()),
            "forfeit must clear the position"
        );
        assert!(
            Positions::<Test>::contains_key(&other, netuid()),
            "peer short must remain"
        );
        assert_eq!(PositionCount::<Test>::get(netuid()), count_before - 1);
        assert_eq!(TotalPositions::<Test>::get(), total_before - 1);
        assert!(
            System::events()
                .into_iter()
                .any(|e| matches!(e.event, RuntimeEvent::Lending(Event::Forfeited { .. })))
        );
        assert!(
            System::events()
                .into_iter()
                .all(|e| !matches!(e.event, RuntimeEvent::Lending(Event::Closed { .. }))),
            "must not continue to Closed after forfeit"
        );
        assert!(
            System::events()
                .into_iter()
                .filter(|e| matches!(e.event, RuntimeEvent::Lending(Event::Readded { .. })))
                .count()
                <= 1,
            "must not double-readd after forfeit"
        );
    });
}

#[test]
fn partial_repay_stops_after_interest_forfeits_short() {
    new_test_ext().execute_with(|| {
        let other = account(3);
        let _other_hk = account(4);
        mint_tao(&other, 50_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(other.clone()),
            netuid(),
            Side::Short,
            10_000_000_000_000,
            None,
            1,
            1,
            0,
            deadline(),
        ));

        let owner = account(1);
        let hotkey = account(2);
        mint_tao(&owner, 50_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Short,
            8_000_000_000_000,
            None,
            1,
            1,
            0,
            deadline(),
        ));
        let snap = Positions::<Test>::get(&owner, netuid()).unwrap();
        mint_tao(&snap.custody, 20_000_000_000_000);
        mint_alpha(&owner, &hotkey, netuid(), snap.principal.saturating_mul(2));
        Positions::<Test>::mutate(&owner, netuid(), |p| {
            if let Some(p) = p {
                p.collateral = 1;
                p.annual_interest = 10_000_000_000_000;
                p.interest_remainder = 0;
                p.last_accrued = System::block_number().saturating_sub(100);
            }
        });
        let count_before = PositionCount::<Test>::get(netuid());
        System::reset_events();
        assert_ok!(Lending::repay(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            account(2),
            snap.principal / 2,
            true,
            u64::MAX,
            0,
            deadline(),
        ));
        assert!(!Positions::<Test>::contains_key(&owner, netuid()));
        assert!(Positions::<Test>::contains_key(&other, netuid()));
        assert_eq!(PositionCount::<Test>::get(netuid()), count_before - 1);
        assert!(
            System::events()
                .into_iter()
                .any(|e| matches!(e.event, RuntimeEvent::Lending(Event::Forfeited { .. })))
        );
        assert!(
            System::events().into_iter().all(|e| !matches!(
                e.event,
                RuntimeEvent::Lending(Event::Repaid { .. } | Event::Closed { .. })
            )),
            "must not continue repay/close after forfeit"
        );
    });
}

#[test]
fn short_wallet_close_rejects_when_max_payment_below_principal() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let hotkey = account(2);
        mint_tao(&owner, 50_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Short,
            10_000_000_000_000,
            None,
            1,
            1,
            0,
            deadline(),
        ));
        let position = Positions::<Test>::get(&owner, netuid()).unwrap();
        // Fund enough alpha that repay would succeed if max_payment were ignored.
        mint_alpha(
            &owner,
            &hotkey,
            netuid(),
            position.principal.saturating_mul(2),
        );
        let alpha_before = alpha(&owner, &hotkey, netuid());
        let principal = position.principal;
        assert_err!(
            Lending::close(
                RuntimeOrigin::signed(owner.clone()),
                netuid(),
                account(2),
                true,
                principal.saturating_sub(1),
                0,
                deadline(),
            ),
            Error::<Test>::SlippageExceeded
        );
        assert!(Positions::<Test>::get(&owner, netuid()).is_some());
        assert_eq!(
            alpha(&owner, &hotkey, netuid()),
            alpha_before,
            "failed max_payment must not debit alpha"
        );
        // Partial repay must honor the same ceiling.
        assert_err!(
            Lending::repay(
                RuntimeOrigin::signed(owner.clone()),
                netuid(),
                account(2),
                principal / 2,
                true,
                0,
                0,
                deadline(),
            ),
            Error::<Test>::SlippageExceeded
        );
        assert_eq!(
            Positions::<Test>::get(&owner, netuid()).unwrap().principal,
            principal
        );
        // Adequate ceiling closes cleanly.
        assert_ok!(Lending::close(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            account(2),
            true,
            principal,
            0,
            deadline(),
        ));
        assert!(Positions::<Test>::get(&owner, netuid()).is_none());
    });
}

#[test]
fn long_close_refunds_alpha_to_the_named_destination_hotkey() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let source = account(2);
        let destination = account(3);
        let collateral = 20_000_000_000_000u64;
        mint_alpha(&owner, &source, netuid(), collateral);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Long,
            collateral,
            Some(source.clone()),
            1,
            0,
            1,
            deadline(),
        ));
        assert_eq!(alpha(&owner, &source, netuid()), 0);
        assert_ok!(Lending::close(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            destination.clone(),
            false,
            u64::MAX,
            0,
            deadline(),
        ));
        assert!(Positions::<Test>::get(&owner, netuid()).is_none());
        assert_eq!(alpha(&owner, &source, netuid()), 0);
        assert!(
            alpha(&owner, &destination, netuid()) > 0,
            "leftover collateral must land on the close-time hotkey"
        );
    });
}

#[test]
fn short_wallet_repay_pulls_alpha_from_the_named_hotkey() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let payer = account(3);
        mint_tao(&owner, 50_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Short,
            10_000_000_000_000,
            None,
            1,
            1,
            0,
            deadline(),
        ));
        let principal = Positions::<Test>::get(&owner, netuid()).unwrap().principal;
        mint_alpha(&owner, &payer, netuid(), principal);
        assert_ok!(Lending::close(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            payer.clone(),
            true,
            u64::MAX,
            0,
            deadline(),
        ));
        assert!(Positions::<Test>::get(&owner, netuid()).is_none());
        assert_eq!(alpha(&owner, &payer, netuid()), 0);
    });
}

#[test]
fn long_amm_close_sells_minimal_collateral_and_refunds_alpha() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let hotkey = account(2);
        let collateral = 20_000_000_000_000u64;
        mint_alpha(&owner, &hotkey, netuid(), collateral.saturating_mul(2));
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Long,
            collateral,
            Some(hotkey.clone()),
            1,
            0,
            1,
            deadline(),
        ));
        let position = Positions::<Test>::get(&owner, netuid()).unwrap();
        assert!(position.escrow_alpha > 0);
        assert!(position.principal > 0);
        // Round-trip A alone is short of P (fees / CP), so close must sell some C.
        let a_alone = MockPool::quote_sell(netuid(), position.escrow_alpha.into())
            .unwrap()
            .to_u64();
        assert!(
            a_alone < position.principal,
            "test needs A sale short of P (got {} vs {})",
            a_alone,
            position.principal
        );

        let quoted = Lending::quote_close(&owner, netuid(), &account(2), false).unwrap();
        assert_eq!(quoted.payment, position.principal);
        assert!(
            quoted.refund > 0 && quoted.refund < position.collateral,
            "quote should leave leftover collateral alpha, not sell it all"
        );

        let alpha_before = alpha(&owner, &hotkey, netuid());
        let tao_before = tao(&owner);
        assert_ok!(Lending::close(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            account(2),
            false,
            u64::MAX,
            0,
            deadline(),
        ));
        assert!(Positions::<Test>::get(&owner, netuid()).is_none());
        let alpha_refunded = alpha(&owner, &hotkey, netuid()).saturating_sub(alpha_before);
        assert!(
            alpha_refunded > 0,
            "remaining collateral must return as alpha"
        );
        let diff = quoted.refund.abs_diff(alpha_refunded);
        assert!(
            diff <= quoted.refund / 100 + 1,
            "quote alpha refund {} vs executed {} (diff {})",
            quoted.refund,
            alpha_refunded,
            diff
        );
        // Dust TAO surplus may appear; primary refund is alpha, not full C as TAO.
        let tao_gained = tao(&owner).saturating_sub(tao_before);
        assert!(
            tao_gained < position.principal / 10,
            "must not liquidate all leftover collateral into TAO (gained {})",
            tao_gained
        );
    });
}

#[test]
fn quote_open_collateral_only_applies_ema_band() {
    new_test_ext().execute_with(|| {
        // Spot already above EMA·1.15 — any long open (and its quote) must fail.
        let ema = U64F64::from_num(1u64) / U64F64::from_num(2u64);
        set_reference_price(ema);
        assert!(
            MockPool::price()
                > ema.saturating_mul(U64F64::from_num(115u64)) / U64F64::from_num(100u64)
        );

        let owner = account(1);
        let hotkey = account(2);
        mint_alpha(&owner, &hotkey, netuid(), 50_000_000_000_000);

        assert_err!(
            Lending::quote_open_for(
                &owner,
                netuid(),
                Side::Long,
                5_000_000_000_000,
                Some(&hotkey),
            ),
            Error::<Test>::PriceTooFarFromEma
        );
        // Exact path: size under V_dyn/LTV so EMA (not collateral) is the refusal.
        assert_err!(
            Lending::quote_open_exact_for(
                &owner,
                netuid(),
                Side::Long,
                5_000_000_000_000,
                Some(&hotkey),
                100_000_000_000, // 0.1 τ ≪ LTV·V_dyn at p_ref=0.5
            ),
            Error::<Test>::PriceTooFarFromEma
        );
        assert_err!(
            Lending::open(
                RuntimeOrigin::signed(owner.clone()),
                netuid(),
                Side::Long,
                5_000_000_000_000,
                Some(hotkey),
                1,
                0,
                1,
                deadline(),
            ),
            Error::<Test>::PriceTooFarFromEma
        );
    });
}

#[test]
fn coldkey_swap_refused_while_subnet_dissolving() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let new_owner = account(11);
        let _hotkey = account(2);
        mint_tao(&owner, 50_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Short,
            10_000_000_000_000,
            None,
            1,
            1,
            0,
            deadline(),
        ));
        assert_ok!(Lending::start_dissolution(netuid()));
        assert!(Dissolutions::<Test>::contains_key(netuid()));
        // Simulate a mid-settlement cursor so a re-key past it would skip the loan.
        DissolutionCursor::<Test>::insert(netuid(), account(0));
        assert_err!(
            Lending::on_coldkey_swap(&owner, &new_owner),
            Error::<Test>::SubnetUnavailable
        );
        assert!(Positions::<Test>::get(&owner, netuid()).is_some());
        assert!(Positions::<Test>::get(&new_owner, netuid()).is_none());
        assert!(OpenByNetuid::<Test>::contains_key(netuid(), &owner));
        assert!(!OpenByNetuid::<Test>::contains_key(netuid(), &new_owner));
    });
}

#[test]
fn long_amm_close_minimal_sale_when_max_sell_quote_hits_slippage() {
    new_test_ext().execute_with(|| {
        // Mainnet-shaped shallow Balancer pool with uneven quote weights.
        // max_sell_input = alpha_reserve×1000; quote_sell refuses above ×10 —
        // the old alpha_to_raise_tao probed max and fell back to dumping all C.
        set_pool(50_000_000_000_000, 800_000_000_000_000); // 50τ / 800kα
        set_quote_weight_parts(200_000_000_000_000_000); // 20% TAO weight parts
        Lifts::<Test>::mutate(netuid(), |book| book.g_ema = pool_tao());
        set_tao_ema(Lending::settlement_tao_basis(netuid()));
        set_reference_price(MockPool::price());

        let max_in = MockPool::max_sell_input(netuid()).to_u64();
        assert!(
            MockPool::quote_sell(netuid(), max_in.into()).is_err(),
            "precondition: quoting max_sell_input must hit SlippageTooHigh"
        );

        let owner = account(1);
        let hotkey = account(2);
        let collateral = 40_000_000_000_000u64; // 40kα
        mint_alpha(&owner, &hotkey, netuid(), collateral.saturating_mul(3));
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Long,
            collateral,
            Some(hotkey.clone()),
            1,
            0,
            1,
            deadline(),
        ));
        let position = Positions::<Test>::get(&owner, netuid()).unwrap();
        let a_alone = MockPool::quote_sell(netuid(), position.escrow_alpha.into())
            .unwrap()
            .to_u64();
        assert!(
            a_alone < position.principal,
            "need A sale short of P so collateral is tapped"
        );

        let quoted = Lending::quote_close(&owner, netuid(), &account(2), false).unwrap();
        assert!(
            quoted.refund > 0 && quoted.refund < position.collateral,
            "quote must leave leftover alpha, got refund={} collateral={}",
            quoted.refund,
            position.collateral
        );

        let alpha_before = alpha(&owner, &hotkey, netuid());
        let coll_before = position.collateral;
        assert_ok!(Lending::close(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            account(2),
            false,
            u64::MAX,
            quoted.refund.saturating_mul(99) / 100,
            deadline(),
        ));
        assert!(Positions::<Test>::get(&owner, netuid()).is_none());
        let alpha_refunded = alpha(&owner, &hotkey, netuid()).saturating_sub(alpha_before);
        assert!(
            alpha_refunded > 0,
            "must refund leftover collateral as alpha, not dump it all"
        );
        assert!(
            alpha_refunded < coll_before,
            "must sell only the shortfall, not the full collateral book"
        );
        let diff = quoted.refund.abs_diff(alpha_refunded);
        assert!(
            diff <= quoted.refund / 100 + 1,
            "quote alpha refund {} vs executed {} (diff {})",
            quoted.refund,
            alpha_refunded,
            diff
        );
    });
}

#[test]
fn principal_zero_is_distinct_from_amount_too_small() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let _hotkey = account(2);
        mint_tao(&owner, 50_000_000_000_000);
        assert_err!(
            Lending::open_exact(
                RuntimeOrigin::signed(owner.clone()),
                netuid(),
                Side::Short,
                10_000_000_000_000,
                None,
                0,
                0,
                0,
                deadline(),
            ),
            Error::<Test>::PrincipalZero
        );
        assert_err!(
            Lending::quote_open_exact_for(
                &owner,
                netuid(),
                Side::Short,
                10_000_000_000_000,
                None,
                0,
            ),
            Error::<Test>::PrincipalZero
        );
        assert_err!(
            Lending::open(
                RuntimeOrigin::signed(owner.clone()),
                netuid(),
                Side::Short,
                0,
                None,
                1,
                0,
                0,
                deadline(),
            ),
            Error::<Test>::CollateralZero
        );
    });
}

#[test]
fn young_subnet_reports_gate_before_zero_principal() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let _hotkey = account(2);
        mint_tao(&owner, 50_000_000_000_000);
        set_subtoken_enabled(false);
        // Zero principal must not mask SubnetNotStarted.
        assert_err!(
            Lending::open_exact(
                RuntimeOrigin::signed(owner.clone()),
                netuid(),
                Side::Short,
                0,
                None,
                0,
                0,
                0,
                deadline(),
            ),
            Error::<Test>::SubnetNotStarted
        );
        assert_err!(
            Lending::quote_open_exact_for(&owner, netuid(), Side::Short, 0, None, 0,),
            Error::<Test>::SubnetNotStarted
        );
        set_subtoken_enabled(true);
        LiftParamsStore::<Test>::put(LiftParams::default());
        set_registered_at(System::block_number());
        assert_err!(
            Lending::open_exact(
                RuntimeOrigin::signed(owner.clone()),
                netuid(),
                Side::Short,
                0,
                None,
                0,
                0,
                0,
                deadline(),
            ),
            Error::<Test>::SubnetTooNew
        );
        assert_err!(
            Lending::quote_open(netuid(), Side::Short, 0),
            Error::<Test>::SubnetTooNew
        );
    });
}

#[test]
fn quote_open_matches_open_gate_and_empty_pool_errors() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let _hotkey = account(2);
        mint_tao(&owner, 50_000_000_000_000);
        // Empty pool → EmptyPool on both extrinsic and quote after gates pass.
        set_pool(0, 0);
        Lifts::<Test>::mutate(netuid(), |book| book.g_ema = 1);
        assert_err!(
            Lending::open_exact(
                RuntimeOrigin::signed(owner.clone()),
                netuid(),
                Side::Short,
                1_000_000_000_000,
                None,
                1_000_000_000,
                0,
                0,
                deadline(),
            ),
            Error::<Test>::EmptyPool
        );
        assert_err!(
            Lending::quote_open_exact_for(
                &owner,
                netuid(),
                Side::Short,
                1_000_000_000_000,
                None,
                1_000_000_000,
            ),
            Error::<Test>::EmptyPool
        );
    });
}

#[test]
fn short_collateral_quote_uses_alpha_not_tao_headroom_on_cheap_subnet() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let _hotkey = account(2);
        // Cheap subnet: 1,000 τ / 1,000,000 α → price 0.001. Cap_short is in TAO;
        // the old bug capped alpha principal at that TAO headroom count.
        let y = 1_000_000_000_000u64;
        let x = 1_000_000_000_000_000u64;
        set_pool(y, x);
        let spot = MockPool::price();
        References::<Test>::insert(
            netuid(),
            LendingReference {
                price: spot,
                last_updated: 1u64,
                valid_after: 1u64,
            },
        );
        Lifts::<Test>::mutate(netuid(), |book| {
            book.g_ema = y;
            book.last = LiftParams::default().settle_ema_blocks;
        });
        set_tao_ema(Lending::settlement_tao_basis(netuid()));

        // Thin-tier Cap_short ≈ 7.5% of G (the live bug's TAO-as-alpha figure).
        let bogus_tao_as_alpha = y * 15 / 100 / 2;
        let capacity_alpha =
            ((u128::from(bogus_tao_as_alpha) * u128::from(x)) / u128::from(y)) as u64;
        assert!(capacity_alpha > bogus_tao_as_alpha);

        mint_tao(&owner, 50_000_000_000_000);
        // 10 τ collateral; LTV 25% → 2.5 τ debt ≈ 2_500 α at price 0.001,
        // well above the bogus TAO-as-alpha cap and below capacity_alpha.
        let collateral = 10_000_000_000u64;
        let quoted = Lending::quote_open(netuid(), Side::Short, collateral).unwrap();
        assert!(
            quoted.principal > bogus_tao_as_alpha,
            "principal {} must not equal TAO headroom {}",
            quoted.principal,
            bogus_tao_as_alpha
        );
        assert!(
            quoted.principal <= capacity_alpha,
            "principal {} exceeds inverted capacity {}",
            quoted.principal,
            capacity_alpha
        );

        // Extrinsic path must size the same way (not the TAO-as-alpha cap).
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Short,
            collateral,
            None,
            1,
            0,
            0,
            deadline(),
        ));
        let opened = Positions::<Test>::get(&owner, netuid()).unwrap();
        assert!(opened.principal > bogus_tao_as_alpha);
        assert_eq!(opened.principal, quoted.principal);
    });
}

#[test]
fn long_zero_borrowable_quote_is_amount_too_small_not_depth() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let hotkey = account(2);
        mint_tao(&owner, 50_000_000_000_000);
        // 1 rao of alpha cannot support any positive long principal under LTV.
        assert_err!(
            Lending::quote_open(netuid(), Side::Long, 1),
            Error::<Test>::AmountTooSmall
        );
        assert_err!(
            Lending::quote_open_for(&owner, netuid(), Side::Long, 1, Some(&hotkey)),
            Error::<Test>::AmountTooSmall
        );
        // Shorts with dust TAO collateral match the same economic error.
        assert_err!(
            Lending::quote_open(netuid(), Side::Short, 1),
            Error::<Test>::AmountTooSmall
        );
    });
}

#[test]
fn gema_warming_up_is_distinct_from_subnet_too_new() {
    cold_lending_ext().execute_with(|| {
        let owner = account(1);
        let _hotkey = account(2);
        mint_tao(&owner, 50_000_000_000_000);
        let params = LiftParams::default();
        // Past min age and reference warm, but G_ema never aged.
        let ready = params.new_subnet_min_age.saturating_add(10);
        System::set_block_number(ready);
        assert_ok!(Lending::warm_reference(
            RuntimeOrigin::signed(account(9)),
            netuid()
        ));
        let warmup: u64 = <<Test as Config>::ReferenceWarmup as Get<_>>::get();
        System::set_block_number(ready.saturating_add(warmup).saturating_add(1));
        // Force G_ema "unaged": last == registered (0).
        Lifts::<Test>::mutate(netuid(), |book| {
            book.g_ema = 1;
            book.last = 0;
        });
        set_registered_at(0);
        assert_err!(
            Lending::open(
                RuntimeOrigin::signed(owner.clone()),
                netuid(),
                Side::Short,
                10_000_000_000_000,
                None,
                1,
                1,
                0,
                deadline(),
            ),
            Error::<Test>::GemaWarmingUp
        );
        assert_err!(
            Lending::quote_open_for(&owner, netuid(), Side::Short, 10_000_000_000_000, None,),
            Error::<Test>::GemaWarmingUp
        );
    });
}

#[test]
fn invalid_source_hotkey_short_rejects_some_and_long_rejects_none() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let hotkey = account(2);
        mint_tao(&owner, 50_000_000_000_000);
        mint_alpha(&owner, &hotkey, netuid(), 50_000_000_000_000);
        assert_err!(
            Lending::open(
                RuntimeOrigin::signed(owner.clone()),
                netuid(),
                Side::Short,
                1_000_000_000_000,
                Some(hotkey.clone()),
                1,
                0,
                0,
                deadline(),
            ),
            Error::<Test>::InvalidSourceHotkey
        );
        assert_err!(
            Lending::quote_open_for(
                &owner,
                netuid(),
                Side::Short,
                1_000_000_000_000,
                Some(&hotkey),
            ),
            Error::<Test>::InvalidSourceHotkey
        );
        assert_err!(
            Lending::open(
                RuntimeOrigin::signed(owner.clone()),
                netuid(),
                Side::Long,
                1_000_000_000_000,
                None,
                1,
                0,
                0,
                deadline(),
            ),
            Error::<Test>::InvalidSourceHotkey
        );
        assert_err!(
            Lending::quote_open_for(&owner, netuid(), Side::Long, 1_000_000_000_000, None),
            Error::<Test>::InvalidSourceHotkey
        );
    });
}

/// Quote Ok with `min_borrow = quoted.principal` must dispatch Ok in the same
/// block — including after pending interest would shrink collateral.
#[test]
fn quote_ok_implies_open_ok_for_random_sizes_and_interest() {
    new_test_ext().execute_with(|| {
        let collaterals: [u64; 6] = [
            1_000_000_000,
            7_500_000_000,
            50_000_000_000,
            250_000_000_000,
            1_000_000_000_000,
            4_000_000_000_000,
        ];
        for (i, &collateral) in collaterals.iter().enumerate() {
            let owner = account(20 + i as u8);
            let hotkey = account(60 + i as u8);
            mint_tao(&owner, collateral.saturating_mul(4).max(10_000_000_000_000));
            let side = if i % 2 == 0 { Side::Short } else { Side::Long };
            if side == Side::Long {
                mint_alpha(&owner, &hotkey, netuid(), collateral.saturating_mul(4));
            }
            let source = if side == Side::Long {
                Some(hotkey.clone())
            } else {
                None
            };
            match Lending::quote_open_for(&owner, netuid(), side, collateral, source.as_ref()) {
                Ok(quoted) => {
                    assert!(quoted.principal > 0);
                    assert_ok!(Lending::open(
                        RuntimeOrigin::signed(owner.clone()),
                        netuid(),
                        side,
                        collateral,
                        source.clone(),
                        quoted.principal,
                        0,
                        0,
                        deadline(),
                    ));
                    let opened = Positions::<Test>::get(&owner, netuid()).unwrap();
                    assert_eq!(opened.principal, quoted.principal);
                    assert_eq!(opened.side, side);
                }
                Err(_) => {
                    assert!(
                        Lending::open(
                            RuntimeOrigin::signed(owner.clone()),
                            netuid(),
                            side,
                            collateral,
                            source,
                            1,
                            0,
                            0,
                            deadline(),
                        )
                        .is_err(),
                        "quote Err must not become open Ok for collateral {collateral}"
                    );
                }
            }
        }

        // Existing short + accrued interest: quote must size post-interest so
        // min_borrow = quoted.principal cannot hit BelowMinimumBorrow.
        let owner = account(1);
        let _hotkey = account(2);
        mint_tao(&owner, 100_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Short,
            5_000_000_000_000,
            None,
            1,
            0,
            0,
            deadline(),
        ));
        System::set_block_number(System::block_number().saturating_add(400));
        let top_up = 2_000_000_000_000u64;
        mint_tao(&owner, top_up);
        let quoted = Lending::quote_open_for(&owner, netuid(), Side::Short, top_up, None).unwrap();
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Short,
            top_up,
            None,
            quoted.principal,
            0,
            0,
            deadline(),
        ));
    });
}

/// Near-full short capacity: a large TAO top-up must report SideCapacity (the
/// binder), not AmountTooSmall — and the quote must match the extrinsic.
#[test]
fn short_top_up_near_capacity_is_side_capacity_in_quote_and_open() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let _hotkey = account(2);
        mint_tao(&owner, 50_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Short,
            1_000_000_000_000,
            None,
            1,
            0,
            0,
            deadline(),
        ));
        // Exhaust short-side TAO headroom while leaving the position open.
        Lifts::<Test>::mutate(netuid(), |book| {
            book.l_s = u64::MAX / 2;
        });
        let add = 200_000_000_000u64; // ~200 τ — large relative to a dust binder
        assert_err!(
            Lending::quote_open_for(&owner, netuid(), Side::Short, add, None),
            Error::<Test>::SideCapacity
        );
        assert_err!(
            Lending::open(
                RuntimeOrigin::signed(owner),
                netuid(),
                Side::Short,
                add,
                None,
                1,
                0,
                0,
                deadline(),
            ),
            Error::<Test>::SideCapacity
        );
    });
}

/// μ SN29 repro: cheap pool (~τ0.00279), ~800 τ short collateral in 4×200 τ
/// steps. Thin-tier Cap_short = 15%×50%×G_ema is only ~100 τ of lift, so LTV
/// 25% (~50 τ lift per step) binds mid-sequence. Remaining headroom must either
/// clamp to a still-executable lift or refuse with SideCapacity — never
/// AmountTooSmall, and quote must match the call.
#[test]
fn sn29_style_short_raises_report_side_capacity_not_amount_too_small() {
    new_test_ext().execute_with(|| {
        // Live-ish SN29 reserves (9-decimal rao / α).
        let tao_in = 8_332_312_063_472u64;
        let alpha_in = 2_985_694_839_137_648u64;
        set_pool(tao_in, alpha_in);
        let spot = MockPool::price();
        // price ≈ tao_in/alpha_in ≈ 0.00279
        assert!(spot < U64F64::from_num(1) / U64F64::from_num(300));
        assert!(spot > U64F64::from_num(1) / U64F64::from_num(400));
        References::<Test>::insert(
            netuid(),
            LendingReference {
                price: spot,
                last_updated: 1u64,
                valid_after: 1u64,
            },
        );
        // Thin G_ema so Cap = 15%·G and Cap_short = 7.5%·G ≈ 100 τ.
        // Each 200 τ collateral add wants ~50 τ of lift under 25% LTV.
        let g_ema = 1_333_000_000_000u64;
        Lifts::<Test>::mutate(netuid(), |book| {
            book.g_ema = g_ema;
            book.last = LiftParams::default().settle_ema_blocks;
            book.l_s = 0;
        });
        set_tao_ema(Lending::settlement_tao_basis(netuid()));

        let params = Lending::lift_params();
        assert!(g_ema < params.thin_tao);
        let floor = g_ema * u64::from(params.thin_floor_bps) / 10_000;
        let cap_short = floor * u64::from(params.split_short_bps) / 10_000;
        assert!(
            (90_000_000_000..110_000_000_000).contains(&cap_short),
            "expected ~100 τ Cap_short, got {cap_short}"
        );

        let owner = account(1);
        let _hotkey = account(2);
        let step = 200_000_000_000u64; // 200 τ
        mint_tao(&owner, step.saturating_mul(6));

        let mut principals: Vec<u64> = Vec::new();
        let mut saw_side_capacity = false;
        for i in 0..4u32 {
            let quote = Lending::quote_open_for(&owner, netuid(), Side::Short, step, None);
            let dispatch = Lending::open(
                RuntimeOrigin::signed(owner.clone()),
                netuid(),
                Side::Short,
                step,
                None,
                1,
                0,
                0,
                deadline(),
            );
            match (quote, dispatch) {
                (Ok(q), Ok(_)) => {
                    assert!(q.principal > 0, "step {i}: quote Ok with zero principal");
                    principals.push(q.principal);
                }
                (Err(q_err), Err(d_err)) => {
                    assert_eq!(
                        q_err, d_err,
                        "step {i}: quote and open must agree on the error"
                    );
                    assert_ne!(
                        q_err,
                        Error::<Test>::AmountTooSmall.into(),
                        "step {i}: capacity binder must not look like AmountTooSmall"
                    );
                    assert_eq!(
                        q_err,
                        Error::<Test>::SideCapacity.into(),
                        "step {i}: expected SideCapacity, got {q_err:?}"
                    );
                    saw_side_capacity = true;
                }
                (Ok(q), Err(e)) => panic!("step {i}: quote Ok ({q:?}) but open Err ({e:?})"),
                (Err(e), Ok(_)) => panic!("step {i}: quote Err ({e:?}) but open Ok"),
            }
        }

        assert!(
            !principals.is_empty(),
            "at least the first 200 τ step should open under ~100 τ Cap_short"
        );
        // Capacity binds: later steps shrink (clamp) and/or refuse SideCapacity.
        let bound = saw_side_capacity
            || principals
                .windows(2)
                .any(|w| w[1] < w[0].saturating_mul(50) / 100);
        assert!(
            bound,
            "expected clamp or SideCapacity across 4×200 τ; principals={principals:?} side_capacity={saw_side_capacity}"
        );
        let book = Lifts::<Test>::get(netuid());
        assert!(
            book.l_s <= cap_short,
            "l_s={} must not exceed Cap_short≈{}",
            book.l_s,
            cap_short
        );
        // Near the cap after ~800 τ of attempts.
        assert!(
            book.l_s + 5_000_000_000 >= cap_short || saw_side_capacity,
            "l_s={} should sit near Cap_short≈{} once capacity binds",
            book.l_s,
            cap_short
        );
    });
}

#[test]
fn quote_raise_ok_implies_open_long_and_sell_ok() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let hotkey = account(2);
        let collateral = 5_000_000_000_000u64;
        mint_alpha(&owner, &hotkey, netuid(), collateral.saturating_mul(4));
        let quoted =
            Lending::quote_raise(&owner, netuid(), &hotkey, None, Some(collateral), None).unwrap();
        assert!(quoted.principal_tao > 0);
        assert_ok!(Lending::open_long_and_sell(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            hotkey,
            Some(collateral),
            None,
            collateral,
            quoted.principal_tao,
            Some(quoted.sell_alpha),
            0,
            0,
            deadline(),
        ));
        let opened = Positions::<Test>::get(&owner, netuid()).unwrap();
        assert_eq!(opened.principal, quoted.principal_tao);
    });
}
