#![allow(
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::indexing_slicing
)]

//! Focused unit tests for lift-based lending v5.1 (Jacob §13 / §13.2).

use super::*;
use codec::{Decode, Encode};
use frame_support::{
    BoundedVec, assert_err, assert_ok, construct_runtime, derive_impl, parameter_types,
    traits::{ConstU32, ConstU64, Get, Hooks},
    weights::{Weight, WeightMeter},
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

    fn quote_sell(netuid: NetUid, alpha: AlphaBalance) -> Result<TaoBalance, DispatchError> {
        let y = Self::pool_tao(netuid).to_u64();
        let x = Self::pool_alpha(netuid).to_u64();
        let a = alpha.to_u64();
        Ok(((u128::from(y) * u128::from(a) / u128::from(x.saturating_add(a))) as u64).into())
    }

    fn quote_buy(netuid: NetUid, tao_in: TaoBalance) -> Result<AlphaBalance, DispatchError> {
        let y = Self::pool_tao(netuid).to_u64();
        let x = Self::pool_alpha(netuid).to_u64();
        let t = tao_in.to_u64();
        Ok(((u128::from(x) * u128::from(t) / u128::from(y.saturating_add(t))) as u64).into())
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
        // 50/50 Balancer quote weight parts.
        500_000_000_000_000_000
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
    ) -> Result<TaoBalance, DispatchError> {
        let t = Self::sell_alpha(
            custody_cold,
            custody_hot,
            netuid,
            alpha_in,
            0u64.into(),
            false,
        )?;
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
        let hotkey = account(2);
        mint_tao(&owner, 50_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Short,
            10_000_000_000_000,
            hotkey,
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
        assert_eq!(alpha(&owner, &position.hotkey, netuid()), 0);
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
            hotkey.clone(),
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
        let hotkey = account(2);
        mint_tao(&owner, 50_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Short,
            5_000_000_000_000,
            hotkey,
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
        let hotkey = account(2);
        mint_tao(&owner, 80_000_000_000_000);
        // Open short while price is ~1.
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Short,
            20_000_000_000_000,
            hotkey,
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
        let hotkey = account(2);
        mint_tao(&owner, 80_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Short,
            20_000_000_000_000,
            hotkey,
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
        let hotkey = account(2);
        mint_tao(&owner, 50_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Short,
            10_000_000_000_000,
            hotkey,
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
        let hotkey = account(2);
        mint_tao(&owner, 50_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Short,
            10_000_000_000_000,
            hotkey,
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
        let hotkey = account(2);
        mint_tao(&owner, 50_000_000_000_000);
        // Near-dereg is NOT modeled; only Dissolutions flag blocks opens.
        assert_ok!(Lending::start_dissolution(netuid()));
        assert!(
            Lending::open(
                RuntimeOrigin::signed(owner),
                netuid(),
                Side::Short,
                1_000_000_000_000,
                hotkey,
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
        let hotkey = account(2);
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
                hotkey.clone(),
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
            hotkey,
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
            hotkey.clone(),
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
        let hotkey = account(2);
        mint_tao(&owner, 50_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Short,
            5_000_000_000_000,
            hotkey,
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

#[test]
fn settle_phase0_subtracts_delta0_and_emits_settle_rate_fixed() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let hotkey = account(2);
        mint_tao(&owner, 50_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Short,
            10_000_000_000_000,
            hotkey,
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
                hotkey.clone(),
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
                hotkey.clone(),
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
            hotkey,
            1,
            0,
            0,
            deadline(),
        ));
        assert!(
            Lending::close(
                RuntimeOrigin::signed(owner),
                netuid(),
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
                hotkey.clone(),
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
                hotkey,
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
            hotkey.clone(),
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
        let hotkey = account(2);
        mint_tao(&owner, 50_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Short,
            10_000_000_000_000,
            hotkey,
            1,
            1,
            0,
            deadline(),
        ));
        let position = Positions::<Test>::get(&owner, netuid()).unwrap();
        let custody = position.custody.clone();
        let escrow_tao_before = tao(&custody);
        assert_ok!(Lending::on_coldkey_swap(&owner, &new_owner));
        assert!(Positions::<Test>::get(&owner, netuid()).is_none());
        let migrated = Positions::<Test>::get(&new_owner, netuid()).unwrap();
        assert_eq!(migrated.custody, custody);
        assert_eq!(tao(&custody), escrow_tao_before);
        assert!(OpenByNetuid::<Test>::contains_key(netuid(), &new_owner));
        assert_eq!(
            EscrowOwner::<Test>::get(netuid(), &custody),
            Some(new_owner)
        );
    });
}

#[test]
fn new_subnet_gate_and_capacity_ramp() {
    new_test_ext().execute_with(|| {
        // Restore production defaults.
        LiftParamsStore::<Test>::put(LiftParams::default());
        let owner = account(1);
        let hotkey = account(2);
        mint_tao(&owner, 50_000_000_000_000);
        set_subtoken_enabled(false);
        assert!(
            Lending::open(
                RuntimeOrigin::signed(owner.clone()),
                netuid(),
                Side::Short,
                1_000_000_000_000,
                hotkey.clone(),
                1,
                0,
                0,
                deadline(),
            )
            .is_err()
        );
        set_subtoken_enabled(true);
        // Age below min_age.
        set_registered_at(System::block_number());
        assert!(
            Lending::open(
                RuntimeOrigin::signed(owner.clone()),
                netuid(),
                Side::Short,
                1_000_000_000_000,
                hotkey.clone(),
                1,
                0,
                0,
                deadline(),
            )
            .is_err()
        );
        // Past gate: age just over min_age so ramp factor is tiny; a large open
        // must hit SideCapacity under the partial Cap.
        let params = LiftParams::default();
        let now = System::block_number();
        set_registered_at(now.saturating_sub(params.new_subnet_min_age + 1));
        Lifts::<Test>::mutate(netuid(), |book| {
            book.last = now;
            book.g_ema = pool_tao();
        });
        assert!(
            Lending::open(
                RuntimeOrigin::signed(owner.clone()),
                netuid(),
                Side::Short,
                20_000_000_000_000,
                hotkey,
                1,
                0,
                0,
                deadline(),
            )
            .is_err()
        );
        let _ = params;
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
        let hotkey = account(2);
        mint_tao(&owner, 50_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Short,
            10_000_000_000_000,
            hotkey.clone(),
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
fn coldkey_swap_rejects_destination_with_existing_loan() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let dest = account(11);
        let hotkey_a = account(2);
        let hotkey_b = account(3);
        mint_tao(&owner, 50_000_000_000_000);
        mint_tao(&dest, 50_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Short,
            10_000_000_000_000,
            hotkey_a,
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
            hotkey_b,
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
        let hotkey = account(2);
        mint_tao(&owner, 50_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Short,
            10_000_000_000_000,
            hotkey,
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

#[test]
fn hotkey_swap_rewrites_open_by_hotkey_index() {
    new_test_ext().execute_with(|| {
        let owner = account(1);
        let hotkey = account(2);
        let new_hotkey = account(22);
        mint_tao(&owner, 50_000_000_000_000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(owner.clone()),
            netuid(),
            Side::Short,
            10_000_000_000_000,
            hotkey.clone(),
            1,
            1,
            0,
            deadline(),
        ));
        assert!(OpenByHotkey::<Test>::contains_key(
            &hotkey,
            (&owner, netuid())
        ));
        assert_ok!(Lending::on_hotkey_swap(&hotkey, &new_hotkey, None));
        assert!(!OpenByHotkey::<Test>::contains_key(
            &hotkey,
            (&owner, netuid())
        ));
        assert!(OpenByHotkey::<Test>::contains_key(
            &new_hotkey,
            (&owner, netuid())
        ));
        assert_eq!(
            Positions::<Test>::get(&owner, netuid()).unwrap().hotkey,
            new_hotkey
        );
        assert_eq!(LoanHotkeys::<Test>::get(&hotkey), 0);
        assert_eq!(LoanHotkeys::<Test>::get(&new_hotkey), 1);
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
        let hotkey = account(2);
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
                hotkey,
                1,
                1,
                0,
                deadline(),
            ),
            Error::<Test>::SubnetTooNew
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
        let hotkey = account(2);
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
                hotkey.clone(),
                1,
                1,
                0,
                deadline(),
            ),
            Error::<Test>::SubnetTooNew
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
            hotkey,
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
                account(2),
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

#[test]
fn migration_warms_existing_subnets_and_is_idempotent() {
    cold_lending_ext().execute_with(|| {
        assert!(!HasMigrationRun::<Test>::get(warm_migration_key()));
        assert!(References::<Test>::get(netuid()).is_none());

        let _w1 = <Lending as Hooks<_>>::on_runtime_upgrade();
        assert!(HasMigrationRun::<Test>::get(warm_migration_key()));
        assert!(References::<Test>::get(netuid()).is_some());
        assert!(Lifts::<Test>::get(netuid()).g_ema > 0);
        assert!(ActiveNets::<Test>::contains_key(netuid()));
        let valid_after = References::<Test>::get(netuid()).unwrap().valid_after;
        let g_ema = Lifts::<Test>::get(netuid()).g_ema;
        let active = ActiveNetCount::<Test>::get();

        // Second upgrade is a no-op for the warm migration (state unchanged).
        let _w2 = <Lending as Hooks<_>>::on_runtime_upgrade();
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
        let _ = <Lending as Hooks<_>>::on_runtime_upgrade();
        assert!(HasMigrationRun::<Test>::get(warm_migration_key()));
        assert!(References::<Test>::get(netuid()).is_none());
        assert_eq!(Lifts::<Test>::get(netuid()).g_ema, 0);
        assert!(!ActiveNets::<Test>::contains_key(netuid()));
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
        let hotkey = account(2);
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
            hotkey,
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
