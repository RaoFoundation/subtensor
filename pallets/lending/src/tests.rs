#![allow(
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::indexing_slicing
)]

use super::*;
use codec::{Decode, Encode};
use frame_support::{
    assert_noop, assert_ok, construct_runtime, derive_impl, parameter_types,
    traits::{ConstU32, ConstU64, Hooks},
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
    type ReferenceWarmup = ConstU64<7200>;
    type MaxPositionsPerSubnet = ConstU32<4>;
    type MaxTotalPositions = ConstU32<5>;
    type MaxFundedSubnets = ConstU32<4>;
    type WeightInfo = TestWeights;
}

pub struct TestWeights;
impl weights::WeightInfo for TestWeights {
    fn open() -> Weight {
        Weight::from_parts(1, 0)
    }
    fn close() -> Weight {
        Weight::from_parts(1, 0)
    }
    fn set_enabled() -> Weight {
        Weight::from_parts(1, 0)
    }
    fn collect() -> Weight {
        Weight::from_parts(1, 0)
    }
    fn update_reference() -> Weight {
        Weight::from_parts(1, 0)
    }
    fn settle() -> Weight {
        Weight::from_parts(1, 0)
    }
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
fn read<V: Decode + Default>(key: &[u8]) -> V {
    sp_io::storage::get(key)
        .and_then(|bytes| V::decode(&mut &bytes[..]).ok())
        .unwrap_or_default()
}
fn write(key: &[u8], value: impl Encode) {
    sp_io::storage::set(key, &value.encode());
}
fn tao(who: &Account) -> u64 {
    read(&key(b"test/tao", who))
}
fn burn_account() -> Account {
    account(255)
}
fn burned_tao() -> u64 {
    tao(&burn_account())
}
fn alpha(who: &Account, hotkey: &Account, netuid: NetUid) -> u64 {
    read(&key(b"test/alpha", (who, hotkey, netuid)))
}
fn mint_tao(who: &Account, amount: u64) {
    write(&key(b"test/tao", who), tao(who) + amount);
}
fn mint_alpha(who: &Account, hotkey: &Account, netuid: NetUid, amount: u64) {
    write(
        &key(b"test/alpha", (who, hotkey, netuid)),
        alpha(who, hotkey, netuid) + amount,
    );
}
fn market(netuid: NetUid) -> (u64, u64) {
    read(&key(b"test/market", netuid))
}
fn swap_count() -> u64 {
    read(b"test/swaps")
}
fn fail_transfer() -> bool {
    read(b"test/fail_transfer")
}

pub struct MockPool;
impl MockPool {
    fn buy_quote(netuid: NetUid, gross: u64) -> Result<u64, DispatchError> {
        ensure!(
            !read::<bool>(b"test/fail_buy"),
            DispatchError::Other("buy unavailable")
        );
        let (a, t) = market(netuid);
        let maximum: u64 = read(b"test/max_buy");
        ensure!(
            gross > 0 && (maximum == 0 || gross <= maximum),
            DispatchError::Other("buy input boundary")
        );
        let net = gross - gross / 1000;
        let output = (u128::from(a) * u128::from(net) / u128::from(t + net)) as u64;
        ensure!(
            !read::<bool>(b"test/reject_dust") || output > 0,
            DispatchError::Other("rounded zero payout")
        );
        Ok(output)
    }
    fn sell_quote(netuid: NetUid, gross: u64) -> Result<u64, DispatchError> {
        ensure!(
            !read::<bool>(b"test/fail_sell"),
            DispatchError::Other("sell unavailable")
        );
        let (a, t) = market(netuid);
        let maximum: u64 = read(b"test/max_sell");
        ensure!(
            gross > 0 && (maximum == 0 || gross <= maximum),
            DispatchError::Other("sell input boundary")
        );
        let net = gross - gross / 1000;
        Ok((u128::from(t) * u128::from(net) / u128::from(a + net)) as u64)
    }
}
impl OrderSwapInterface<Account> for MockPool {
    #[cfg(feature = "runtime-benchmarks")]
    fn set_up_netuid_for_benchmark(netuid: NetUid) {
        write(
            &key(b"test/market", netuid),
            (1_000_000_000_000_u64, 1_000_000_000_000_u64),
        );
    }
    #[cfg(feature = "runtime-benchmarks")]
    fn set_up_acc_for_benchmark(hotkey: &Account, coldkey: &Account) {
        mint_tao(coldkey, 1_000_000_000_000);
        assert_ok!(Self::register_pallet_hotkey(coldkey, hotkey));
    }
    fn buy_alpha(
        coldkey: &Account,
        hotkey: &Account,
        netuid: NetUid,
        amount: TaoBalance,
        _: TaoBalance,
        _: bool,
    ) -> Result<AlphaBalance, DispatchError> {
        let gross = amount.to_u64();
        let out = Self::buy_quote(netuid, gross)?;
        ensure!(
            tao(coldkey) >= gross,
            DispatchError::Other("tao unavailable")
        );
        write(&key(b"test/tao", coldkey), tao(coldkey) - gross);
        mint_alpha(coldkey, hotkey, netuid, out);
        let (a, t) = market(netuid);
        write(
            &key(b"test/market", netuid),
            (a - out, t + gross - gross / 1000),
        );
        write(b"test/swaps", swap_count() + 1);
        Ok(out.into())
    }
    fn sell_alpha(
        coldkey: &Account,
        hotkey: &Account,
        netuid: NetUid,
        amount: AlphaBalance,
        _: TaoBalance,
        _: bool,
    ) -> Result<TaoBalance, DispatchError> {
        let gross = amount.to_u64();
        let out = Self::sell_quote(netuid, gross)?;
        ensure!(
            alpha(coldkey, hotkey, netuid) >= gross,
            DispatchError::Other("alpha unavailable")
        );
        write(
            &key(b"test/alpha", (coldkey, hotkey, netuid)),
            alpha(coldkey, hotkey, netuid) - gross,
        );
        mint_tao(coldkey, out);
        let (a, t) = market(netuid);
        write(
            &key(b"test/market", netuid),
            (a + gross - gross / 1000, t - out),
        );
        write(b"test/swaps", swap_count() + 1);
        Ok(out.into())
    }
    fn current_alpha_price(netuid: NetUid) -> U64F64 {
        let (a, t) = market(netuid);
        U64F64::from_num(t) / U64F64::from_num(a)
    }
    fn transfer_tao(from: &Account, to: &Account, amount: TaoBalance) -> DispatchResult {
        ensure!(!fail_transfer(), DispatchError::Other("transfer failed"));
        let n = amount.to_u64();
        ensure!(tao(from) >= n, DispatchError::Other("tao unavailable"));
        write(&key(b"test/tao", from), tao(from) - n);
        mint_tao(to, n);
        Ok(())
    }
    fn transfer_staked_alpha(
        from: &Account,
        from_hot: &Account,
        to: &Account,
        to_hot: &Account,
        netuid: NetUid,
        amount: AlphaBalance,
        _: bool,
        _: bool,
    ) -> DispatchResult {
        ensure!(!fail_transfer(), DispatchError::Other("transfer failed"));
        let n = amount.to_u64();
        ensure!(
            alpha(from, from_hot, netuid) >= n,
            DispatchError::Other("alpha unavailable")
        );
        write(
            &key(b"test/alpha", (from, from_hot, netuid)),
            alpha(from, from_hot, netuid) - n,
        );
        mint_alpha(to, to_hot, netuid, n);
        Ok(())
    }
    fn register_pallet_hotkey(coldkey: &Account, hotkey: &Account) -> DispatchResult {
        let owner: Option<Account> = read(&key(b"test/owner", hotkey));
        ensure!(
            owner.as_ref().is_none_or(|owner| owner == coldkey),
            DispatchError::Other("hotkey taken")
        );
        write(&key(b"test/owner", hotkey), Some(coldkey.clone()));
        Ok(())
    }
    fn pallet_hotkey_registered(coldkey: &Account, hotkey: &Account) -> bool {
        read::<Option<Account>>(&key(b"test/owner", hotkey)).as_ref() == Some(coldkey)
    }
}
impl LendingPoolInterface<Account> for MockPool {
    fn burn_interest_tao(account: &Account, amount: TaoBalance) -> DispatchResult {
        ensure!(
            !read::<bool>(b"test/fail_burn"),
            DispatchError::Other("burn unavailable")
        );
        let amount = amount.to_u64();
        ensure!(
            tao(account) >= amount,
            DispatchError::Other("tao unavailable")
        );
        write(&key(b"test/tao", account), tao(account) - amount);
        mint_tao(&burn_account(), amount);
        Ok(())
    }
    fn buy_spendable_tao(account: &Account) -> TaoBalance {
        tao(account).saturating_sub(read::<u64>(b"test/ed")).into()
    }
    fn refund_dissolution_tao(
        from: &Account,
        to: &Account,
        amount: TaoBalance,
    ) -> Result<TaoBalance, DispatchError> {
        if tao(to) == 0 && amount.to_u64() < read::<u64>(b"test/ed") {
            ensure!(
                tao(from) >= amount.to_u64(),
                DispatchError::Other("tao unavailable")
            );
            write(&key(b"test/tao", from), tao(from) - amount.to_u64());
            return Ok(TaoBalance::ZERO);
        }
        Self::transfer_tao(from, to, amount)?;
        Ok(amount)
    }
    fn collect_interest_tao(
        from: &Account,
        to: &Account,
        amount: TaoBalance,
    ) -> Result<TaoBalance, DispatchError> {
        Self::refund_dissolution_tao(from, to, amount)
    }
    fn owner_allowed(_: &Account) -> bool {
        !read::<bool>(b"test/disputed")
    }
    fn max_buy_input(_: NetUid) -> TaoBalance {
        let maximum: u64 = read(b"test/max_buy");
        if maximum == 0 {
            u64::MAX.into()
        } else {
            maximum.into()
        }
    }
    fn subnet_exists(netuid: NetUid) -> bool {
        market(netuid).0 > 0 && !read::<bool>(&key(b"test/dissolved", netuid))
    }
    fn quote_sell(netuid: NetUid, amount: AlphaBalance) -> Result<TaoBalance, DispatchError> {
        Self::sell_quote(netuid, amount.to_u64()).map(Into::into)
    }
    fn quote_buy(netuid: NetUid, amount: TaoBalance) -> Result<AlphaBalance, DispatchError> {
        Self::buy_quote(netuid, amount.to_u64()).map(Into::into)
    }
    fn return_dissolution_reserves(
        netuid: NetUid,
        account: &Account,
        hotkey: &Account,
        tao_amount: TaoBalance,
        alpha_amount: AlphaBalance,
    ) -> DispatchResult {
        Self::transfer_tao(
            account,
            &crate::Pallet::<Test>::recovery_account(),
            tao_amount,
        )?;
        ensure!(
            alpha(account, hotkey, netuid) >= alpha_amount.to_u64(),
            DispatchError::Other("alpha unavailable")
        );
        write(
            &key(b"test/alpha", (account, hotkey, netuid)),
            alpha(account, hotkey, netuid) - alpha_amount.to_u64(),
        );
        write(
            b"test/returned",
            (tao_amount.to_u64(), alpha_amount.to_u64()),
        );
        Ok(())
    }
}

pub(crate) fn ext() -> sp_io::TestExternalities {
    let storage = frame_system::GenesisConfig::<Test>::default()
        .build_storage()
        .unwrap();
    let mut ext = sp_io::TestExternalities::new(storage);
    ext.execute_with(|| {
        System::set_block_number(1);
        write(
            &key(b"test/market", netuid()),
            (1_000_000_u64, 1_000_000_u64),
        );
        mint_tao(&account(1), 1_000_000);
        mint_alpha(&account(1), &account(1), netuid(), 1_000_000);
        assert_ok!(Lending::initialize_custody());
        let vault = Lending::reserve_account(netuid());
        let hot = Lending::custody_hotkey().unwrap();
        mint_tao(&vault, 100_000);
        mint_alpha(&vault, &hot, netuid(), 100_000);
        assert_ok!(Lending::fund_reserves(
            netuid(),
            100_000.into(),
            100_000.into(),
            U64F64::from_num(1),
            true
        ));
        assert_ok!(Lending::set_enabled(RuntimeOrigin::root(), true));
    });
    ext
}
fn open(side: Side, collateral: u64) {
    assert_ok!(Lending::open(
        RuntimeOrigin::signed(account(1)),
        netuid(),
        side,
        collateral,
        account(1),
        1,
        0,
    ));
}
fn position() -> Position<Account, u64> {
    Positions::<Test>::get(account(1), netuid()).unwrap()
}
fn idle(now: u64) {
    System::set_block_number(now);
    Lending::on_idle(now, Weight::MAX);
}
fn meter() -> WeightMeter {
    WeightMeter::with_limit(Weight::MAX)
}

#[test]
fn position_accounts_hash_every_owner_byte() {
    let mut second = [1_u8; 32];
    second[31] = 2;
    assert_ne!(
        Lending::position_account(&account(1), netuid()),
        Lending::position_account(&Account::new(second), netuid())
    );
    assert_ne!(
        Lending::position_account(&account(1), netuid()),
        Lending::position_account(&account(1), 65.into())
    );
    assert_ne!(
        Lending::reserve_account(netuid()),
        Lending::recovery_account()
    );
}

#[test]
fn short_is_custodial_and_wallet_repayment_restores_fixed_alpha() {
    ext().execute_with(|| {
        let before = tao(&account(1));
        let before_alpha = alpha(&account(1), &account(1), netuid());
        open(Side::Short, 1000);
        let p = position();
        let escrow = Lending::position_account(&account(1), netuid());
        assert_eq!(p.principal, 250);
        assert_eq!(p.annual_interest, p.proceeds);
        assert_eq!(tao(&account(1)), before - 1000);
        assert_eq!(alpha(&account(1), &account(1), netuid()), before_alpha);
        assert_eq!(tao(&escrow), 1000 + p.proceeds);
        assert_eq!(
            alpha(&escrow, &Lending::custody_hotkey().unwrap(), netuid()),
            0
        );
        assert_ok!(Lending::close(
            RuntimeOrigin::signed(account(1)),
            netuid(),
            true,
            p.principal,
            1000
        ));
        assert!(Positions::<Test>::get(account(1), netuid()).is_none());
        assert_eq!(
            Vaults::<Test>::get(netuid()).unwrap().available_alpha,
            100_000
        );
        assert_eq!(
            alpha(&account(1), &account(1), netuid()),
            before_alpha - p.principal
        );
        assert_eq!(tao(&account(1)), before + p.proceeds);
    });
}

#[test]
fn short_buyback_executes_full_debt_and_quote_matches_close() {
    ext().execute_with(|| {
        open(Side::Short, 1000);
        let q = Lending::quote_close(&account(1), netuid(), false).unwrap();
        let before = tao(&account(1));
        assert_ok!(Lending::close(
            RuntimeOrigin::signed(account(1)),
            netuid(),
            false,
            q.payment,
            q.refund
        ));
        assert_eq!(tao(&account(1)), before + q.refund);
        assert_eq!(Vaults::<Test>::get(netuid()).unwrap().outstanding_alpha, 0);
        assert_eq!(
            Vaults::<Test>::get(netuid()).unwrap().available_alpha,
            100_000
        );
    });
}

#[test]
fn long_tao_is_transferable_and_collateral_returns_in_alpha() {
    ext().execute_with(|| {
        let before_tao = tao(&account(1));
        let before_alpha = alpha(&account(1), &account(1), netuid());
        open(Side::Long, 1000);
        let p = position();
        assert_eq!(tao(&account(1)), before_tao + p.principal);
        assert_eq!(
            alpha(&account(1), &account(1), netuid()),
            before_alpha - 1000
        );
        assert_ok!(MockPool::transfer_tao(
            &account(1),
            &account(2),
            p.principal.into()
        ));
        assert_ok!(Lending::close(
            RuntimeOrigin::signed(account(1)),
            netuid(),
            true,
            p.principal,
            1000
        ));
        assert_eq!(alpha(&account(1), &account(1), netuid()), before_alpha);
        assert_eq!(
            Vaults::<Test>::get(netuid()).unwrap().available_tao,
            100_000
        );
    });
}

#[test]
fn weekly_short_interest_burns_opening_value_coupon_without_enlarging_inventory() {
    ext().execute_with(|| {
        open(Side::Short, 1000);
        let p = position();
        let before = Vaults::<Test>::get(netuid()).unwrap();
        let swaps = swap_count();
        idle(11);
        let after = position();
        let vault = Vaults::<Test>::get(netuid()).unwrap();
        assert_eq!(after.principal, p.principal);
        assert_eq!(after.collateral, 1000 - p.annual_interest * 10 / 520);
        assert_eq!(vault.pending_tao, 0);
        assert_eq!(vault.available_alpha, before.available_alpha);
        assert_eq!(vault.available_tao, before.available_tao);
        assert_eq!(vault.outstanding_alpha, p.principal);
        assert_eq!(burned_tao(), p.annual_interest * 10 / 520);
        assert_eq!(swap_count(), swaps);
    });
}

#[test]
fn failed_short_interest_burn_remains_backed_then_retries() {
    ext().execute_with(|| {
        open(Side::Short, 1000);
        write(b"test/fail_burn", true);
        idle(11);
        let paid = 1000 - position().collateral;
        let vault = Vaults::<Test>::get(netuid()).unwrap();
        assert!(paid > 0);
        assert_eq!(vault.pending_tao, paid);
        assert_eq!(tao(&Lending::reserve_account(netuid())), 100_000 + paid);
        assert_eq!(burned_tao(), 0);
        write(b"test/fail_burn", false);
        idle(12);
        assert_eq!(Vaults::<Test>::get(netuid()).unwrap().pending_tao, 0);
        assert_eq!(burned_tao(), paid);
        assert_eq!(tao(&Lending::reserve_account(netuid())), 100_000);
    });
}

#[test]
fn finite_boundary_sells_only_executable_long_fee_chunk_and_burns_its_tao() {
    ext().execute_with(|| {
        open(Side::Long, 1000);
        write(b"test/max_sell", 2_u64);
        idle(11);
        let vault = Vaults::<Test>::get(netuid()).unwrap();
        assert!(vault.pending_alpha > 0);
        assert!(vault.pending_alpha < 1000 - position().collateral);
        assert_eq!(burned_tao(), 1);
        assert_eq!(vault.available_tao, 100_000 - position().principal);
    });
}

#[test]
fn full_open_and_close_failures_rollback_balances_and_debt() {
    ext().execute_with(|| {
        let before = tao(&account(1));
        assert_noop!(
            Lending::open(
                RuntimeOrigin::signed(account(1)),
                netuid(),
                Side::Short,
                1000,
                account(1),
                251,
                0,
            ),
            Error::<Test>::BelowMinimumBorrow
        );
        assert_eq!(tao(&account(1)), before);
        assert!(Positions::<Test>::get(account(1), netuid()).is_none());
        open(Side::Short, 1000);
        System::set_block_number(12);
        let before_p = position();
        let before_v = Vaults::<Test>::get(netuid());
        assert_noop!(
            Lending::close(RuntimeOrigin::signed(account(1)), netuid(), true, 1, 0),
            Error::<Test>::AboveMaximumPayment
        );
        assert_eq!(position(), before_p);
        assert_eq!(Vaults::<Test>::get(netuid()), before_v);
        write(b"test/fail_transfer", true);
        assert!(
            Lending::close(
                RuntimeOrigin::signed(account(1)),
                netuid(),
                true,
                u64::MAX,
                0
            )
            .is_err()
        );
        assert_eq!(position(), before_p);
        assert_eq!(Vaults::<Test>::get(netuid()), before_v);
    });
}

#[test]
fn annual_interest_exhaustion_forfeits_without_forced_swap() {
    ext().execute_with(|| {
        open(Side::Short, 1000);
        let p = position();
        write(b"test/fail_burn", true);
        let swaps = swap_count();
        idle(1 + 520 * 5);
        assert!(Positions::<Test>::get(account(1), netuid()).is_none());
        let vault = Vaults::<Test>::get(netuid()).unwrap();
        assert_eq!(vault.outstanding_alpha, 0);
        assert_eq!(vault.lost_alpha, p.principal);
        assert_eq!(vault.pending_tao, 1000);
        assert_eq!(vault.available_tao, 100_000 + p.proceeds);
        assert_eq!(burned_tao(), 0);
        assert_eq!(swap_count(), swaps);
        assert_noop!(
            Lending::close(
                RuntimeOrigin::signed(account(1)),
                netuid(),
                true,
                u64::MAX,
                0
            ),
            Error::<Test>::PositionMissing
        );
    });
}

#[test]
fn long_default_does_not_reclaim_transferred_tao() {
    ext().execute_with(|| {
        open(Side::Long, 1000);
        let p = position();
        let user_tao = tao(&account(1));
        write(b"test/fail_sell", true);
        idle(1 + 520 * 5);
        assert_eq!(tao(&account(1)), user_tao);
        let v = Vaults::<Test>::get(netuid()).unwrap();
        assert_eq!(v.lost_tao, p.principal);
        assert_eq!(v.pending_alpha, 1000);
    });
}

#[test]
fn low_ltv_and_aggregate_cap_reject_without_clipping() {
    ext().execute_with(|| {
        assert_eq!(
            Lending::quote_open(netuid(), Side::Short, 4000)
                .unwrap()
                .principal,
            1000
        );
        assert_noop!(
            Lending::open(
                RuntimeOrigin::signed(account(1)),
                netuid(),
                Side::Short,
                44_000,
                account(1),
                0,
                0,
            ),
            Error::<Test>::BorrowingLimit
        );
        assert!(Positions::<Test>::get(account(1), netuid()).is_none());
        open(Side::Short, 40_000);
        let vault = Vaults::<Test>::get(netuid()).unwrap();
        assert_eq!(vault.outstanding_alpha, 10_000);
        assert_eq!(vault.available_alpha + vault.outstanding_alpha, 100_000);
        mint_tao(&account(2), 1000);
        assert_noop!(
            Lending::open(
                RuntimeOrigin::signed(account(2)),
                netuid(),
                Side::Short,
                1000,
                account(2),
                0,
                0,
            ),
            Error::<Test>::BorrowingLimit
        );
    });
}

#[test]
fn reference_is_prior_block_geometric_and_clipped() {
    ext().execute_with(|| {
        write(
            &key(b"test/market", netuid()),
            (1_000_000_u64, 100_000_000_u64),
        );
        assert_eq!(
            References::<Test>::get(netuid()).unwrap().price,
            U64F64::from_num(1)
        );
        System::set_block_number(2);
        Lending::on_finalize(2);
        let p = References::<Test>::get(netuid()).unwrap().price;
        let expected = Lending::ema_update(U64F64::from_num(1), U64F64::from_num(2)).unwrap();
        assert_eq!(p, expected);
        assert!(p < U64F64::from_num(1.001));
        let fixed = p;
        Lending::on_finalize(2);
        assert_eq!(References::<Test>::get(netuid()).unwrap().price, fixed);
    });
}

#[test]
fn twenty_four_hour_geometric_half_life() {
    let mut price = U64F64::from_num(1);
    for _ in 0..7200 {
        price = Lending::ema_update(price, U64F64::from_num(2)).unwrap();
    }
    let actual = price.to_num::<f64>();
    assert!((actual - 2_f64.sqrt()).abs() < 0.000_001);
}

#[test]
fn new_subnet_reference_requires_warmup_and_refunding_does_not_reset_it() {
    ext().execute_with(|| {
        let new: NetUid = 65.into();
        write(&key(b"test/market", new), (100_000_u64, 100_000_u64));
        assert_ok!(Lending::fund_reserves(
            new,
            100_000.into(),
            100_000.into(),
            U64F64::from_num(1),
            false
        ));
        assert!(Lending::quote_open(new, Side::Short, 1000).is_err());
        let reference = References::<Test>::get(new).unwrap();
        assert_ok!(Lending::fund_reserves(
            new,
            1.into(),
            1.into(),
            U64F64::from_num(99),
            true
        ));
        assert_eq!(References::<Test>::get(new).unwrap(), reference);
        System::set_block_number(7201);
        assert!(Lending::quote_open(new, Side::Short, 1000).is_ok());
    });
}

#[test]
fn close_charges_accrued_fraction_before_weekly_boundary() {
    ext().execute_with(|| {
        open(Side::Long, 1000);
        let p = position();
        System::set_block_number(2);
        let q = Lending::quote_close(&account(1), netuid(), true).unwrap();
        assert_eq!(q.refund, 999);
        assert_ok!(Lending::close(
            RuntimeOrigin::signed(account(1)),
            netuid(),
            true,
            p.principal,
            q.refund
        ));
        assert_eq!(Vaults::<Test>::get(netuid()).unwrap().pending_alpha, 1);
    });
}

#[test]
fn fraction_carry_survives_delayed_weekly_collections() {
    ext().execute_with(|| {
        open(Side::Long, 1000);
        let annual = position().annual_interest;
        write(b"test/fail_sell", true);
        for now in [11, 21, 31, 41, 51] {
            idle(now);
        }
        let p = position();
        assert_eq!(p.collateral, 1000 - annual * 50 / 520);
        assert_eq!(p.interest_remainder, annual * 50 % 520);
    });
}

#[test]
fn disabled_borrowing_preserves_repayment() {
    ext().execute_with(|| {
        open(Side::Long, 1000);
        let p = position();
        assert_ok!(Lending::set_enabled(RuntimeOrigin::root(), false));
        assert_ok!(Lending::close(
            RuntimeOrigin::signed(account(1)),
            netuid(),
            true,
            p.principal,
            0
        ));
        assert_noop!(
            Lending::open(
                RuntimeOrigin::signed(account(1)),
                netuid(),
                Side::Long,
                1000,
                account(1),
                0,
                0,
            ),
            Error::<Test>::Disabled
        );
    });
}

#[test]
fn deregistration_uses_common_interest_cutoff_and_no_swaps() {
    ext().execute_with(|| {
        open(Side::Short, 1000);
        let p = position();
        let swaps = swap_count();
        System::set_block_number(6);
        assert_ok!(Lending::start_dissolution(netuid()));
        System::set_block_number(1000);
        assert!(Lending::settle_shorts(netuid(), &mut meter()));
        assert_eq!(swap_count(), swaps);
        assert!(Positions::<Test>::get(account(1), netuid()).is_none());
        let charged = (p.annual_interest * 5).div_ceil(520);
        assert_eq!(burned_tao(), charged);
        let expected_refund = 1000 - charged + p.proceeds - p.principal;
        assert_eq!(tao(&account(1)), 999_000 + expected_refund);
        assert_eq!(Vaults::<Test>::get(netuid()).unwrap().pending_tao, 0);
        assert!(
            Dissolutions::<Test>::get(netuid())
                .unwrap()
                .reserves_returned
        );
    });
}

#[test]
fn deregistration_short_deficit_is_recorded_without_cash_creation() {
    ext().execute_with(|| {
        open(Side::Short, 1000);
        write(
            &key(b"test/market", netuid()),
            (1_000_000_u64, 10_000_000_u64),
        );
        assert_ok!(Lending::start_dissolution(netuid()));
        assert!(Lending::settle_shorts(netuid(), &mut meter()));
        let v = Vaults::<Test>::get(netuid()).unwrap();
        assert!(v.lost_alpha > 0);
        assert_eq!(v.outstanding_alpha, 0);
        assert_eq!(tao(&account(1)), 999_000);
    });
}

#[test]
fn long_deregistration_recovers_only_actual_funded_redemption() {
    for payout in [20_u64, 270] {
        ext().execute_with(|| {
            open(Side::Long, 1000);
            let debt = position().principal;
            let owner_before = tao(&account(1));
            assert_ok!(Lending::start_dissolution(netuid()));
            assert!(Lending::settle_shorts(netuid(), &mut meter()));
            let escrow = Lending::position_account(&account(1), netuid());
            mint_tao(&escrow, payout);
            assert_ok!(Lending::on_alpha_redemption(
                netuid(),
                &escrow,
                payout.into()
            ));
            assert!(Positions::<Test>::contains_key(account(1), netuid()));
            assert!(Lending::settle_remaining_longs(netuid(), &mut meter()));
            let v = Vaults::<Test>::get(netuid()).unwrap();
            assert_eq!(v.available_tao, payout.min(debt));
            assert_eq!(v.lost_tao, debt.saturating_sub(payout));
            assert_eq!(tao(&account(1)), owner_before + payout.saturating_sub(debt));
            assert_ok!(Lending::finish_dissolution(netuid()));
            assert!(Vaults::<Test>::get(netuid()).is_none());
            assert_eq!(tao(&Lending::reserve_account(netuid())), 0);
        });
    }
}

#[test]
fn zero_payout_long_cleanup_prevents_netuid_reuse_with_stale_debt() {
    ext().execute_with(|| {
        open(Side::Long, 1000);
        assert_ok!(Lending::start_dissolution(netuid()));
        assert!(Lending::settle_shorts(netuid(), &mut meter()));
        assert!(Lending::finish_dissolution(netuid()).is_err());
        assert!(Lending::settle_remaining_longs(netuid(), &mut meter()));
        assert_ok!(Lending::finish_dissolution(netuid()));
        assert!(Positions::<Test>::get(account(1), netuid()).is_none());
        assert!(!EscrowOwner::<Test>::contains_key(
            netuid(),
            Lending::position_account(&account(1), netuid())
        ));
    });
}

#[test]
fn metered_deregistration_resumes_past_long_positions() {
    ext().execute_with(|| {
        open(Side::Long, 1000);
        for n in [2, 3] {
            mint_tao(&account(n), 1000);
            mint_alpha(&account(n), &account(n), netuid(), 1000);
            assert_ok!(Lending::open(
                RuntimeOrigin::signed(account(n)),
                netuid(),
                Side::Long,
                1000,
                account(n),
                1,
                0,
            ));
        }
        System::set_block_number(6);
        assert_ok!(Lending::start_dissolution(netuid()));
        System::set_block_number(1000);
        let mut completed = false;
        for _ in 0..4 {
            let mut budget = WeightMeter::with_limit(Weight::from_parts(1, 0));
            completed = Lending::settle_shorts(netuid(), &mut budget);
            if completed {
                break;
            }
        }
        assert!(completed);
        for n in [1, 2, 3] {
            assert_eq!(
                Positions::<Test>::get(account(n), netuid())
                    .unwrap()
                    .last_accrued,
                6
            );
        }
        assert!(!DissolutionCursor::<Test>::contains_key(netuid()));
    });
}

#[test]
fn pumped_spot_cannot_raise_long_credit_above_historical_value() {
    ext().execute_with(|| {
        write(
            &key(b"test/market", netuid()),
            (1_000_000_u64, 100_000_000_u64),
        );
        let quote = Lending::quote_open(netuid(), Side::Long, 1000).unwrap();
        assert_eq!(quote.principal, 250);
        let short = Lending::quote_open(netuid(), Side::Short, 1000).unwrap();
        assert!(short.opening_value <= 250);
        // A buy boundary never disables corrective short openings.
        write(b"test/fail_buy", true);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(account(1)),
            netuid(),
            Side::Short,
            1000,
            account(1),
            short.principal,
            0,
        ));
    });
}

#[test]
fn depressed_spot_caps_long_credit_at_executable_sale_depth() {
    ext().execute_with(|| {
        write(
            &key(b"test/market", netuid()),
            (100_000_000_u64, 1_000_000_u64),
        );
        let quote = Lending::quote_open(netuid(), Side::Long, 1000).unwrap();
        assert!(quote.principal < 250);
        assert_eq!(
            quote.principal,
            MockPool::sell_quote(netuid(), 250).unwrap()
        );
    });
}

#[test]
fn lending_owner_recovery_gate_rejects_new_loans() {
    ext().execute_with(|| {
        write(b"test/disputed", true);
        assert_noop!(
            Lending::open(
                RuntimeOrigin::signed(account(1)),
                netuid(),
                Side::Long,
                1000,
                account(1),
                1,
                0,
            ),
            Error::<Test>::SubnetUnavailable
        );
    });
}

#[test]
fn global_position_cap_applies_across_subnets_and_close_releases_capacity() {
    ext().execute_with(|| {
        let second: NetUid = 65.into();
        write(&key(b"test/market", second), (1_000_000_u64, 1_000_000_u64));
        let vault = Lending::reserve_account(second);
        let hot = Lending::custody_hotkey().unwrap();
        mint_tao(&vault, 100_000);
        mint_alpha(&vault, &hot, second, 100_000);
        assert_ok!(Lending::fund_reserves(
            second,
            100_000.into(),
            100_000.into(),
            U64F64::from_num(1),
            true
        ));
        for n in [1, 2, 3, 4] {
            mint_tao(&account(n), 1000);
            mint_alpha(&account(n), &account(n), netuid(), 1000);
            assert_ok!(Lending::open(
                RuntimeOrigin::signed(account(n)),
                netuid(),
                Side::Long,
                1000,
                account(n),
                1,
                0,
            ));
        }
        mint_alpha(&account(1), &account(1), second, 1000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(account(1)),
            second,
            Side::Long,
            1000,
            account(1),
            1,
            0,
        ));
        assert_eq!(TotalPositions::<Test>::get(), 5);
        mint_alpha(&account(2), &account(2), second, 1000);
        assert_noop!(
            Lending::open(
                RuntimeOrigin::signed(account(2)),
                second,
                Side::Long,
                1000,
                account(2),
                1,
                0,
            ),
            Error::<Test>::TooManyPositions
        );
        assert_ok!(Lending::close(
            RuntimeOrigin::signed(account(1)),
            netuid(),
            true,
            u64::MAX,
            0
        ));
        assert_eq!(TotalPositions::<Test>::get(), 4);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(account(2)),
            second,
            Side::Long,
            1000,
            account(2),
            1,
            0,
        ));
        assert_eq!(TotalPositions::<Test>::get(), 5);
    });
}

#[test]
fn buyback_search_distinguishes_quote_dust_from_upper_boundary() {
    ext().execute_with(|| {
        write(&key(b"test/market", netuid()), (1000_u64, 1_000_000_u64));
        write(b"test/reject_dust", true);
        write(b"test/max_buy", 1500_u64);
        let payment = Lending::buyback_input(netuid(), 1, 2000).unwrap();
        assert!(payment <= 1500);
        assert_eq!(MockPool::buy_quote(netuid(), payment).unwrap(), 1);
        assert!(MockPool::buy_quote(netuid(), payment - 1).is_err());
    });
}

#[test]
fn unbounded_frozen_price_cannot_stall_terminal_short_settlement() {
    ext().execute_with(|| {
        open(Side::Short, 1000);
        let principal = position().principal;
        write(&key(b"test/market", netuid()), (1_u64, u64::MAX));
        assert_ok!(Lending::start_dissolution(netuid()));
        assert!(Lending::settle_shorts(netuid(), &mut meter()));
        let vault = Vaults::<Test>::get(netuid()).unwrap();
        assert_eq!(vault.outstanding_alpha, 0);
        assert_eq!(vault.lost_alpha, principal);
        assert!(Positions::<Test>::get(account(1), netuid()).is_none());
    });
}

#[test]
fn nominated_hotkey_reference_count_covers_every_borrower_until_close() {
    ext().execute_with(|| {
        open(Side::Long, 1000);
        mint_tao(&account(2), 1000);
        mint_alpha(&account(2), &account(1), netuid(), 1000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(account(2)),
            netuid(),
            Side::Long,
            1000,
            account(1),
            1,
            0,
        ));
        assert_eq!(LoanHotkeys::<Test>::get(account(1)), 2);
        assert!(<Lending as LendingInterface<Account>>::has_hotkey_positions(&account(1)));
        assert_ok!(Lending::close(
            RuntimeOrigin::signed(account(1)),
            netuid(),
            true,
            u64::MAX,
            0
        ));
        assert_eq!(LoanHotkeys::<Test>::get(account(1)), 1);
        assert_ok!(Lending::close(
            RuntimeOrigin::signed(account(2)),
            netuid(),
            true,
            u64::MAX,
            0
        ));
        assert!(!LoanHotkeys::<Test>::contains_key(account(1)));
        assert!(!<Lending as LendingInterface<Account>>::has_hotkey_positions(&account(1)));
    });
}

#[test]
fn short_close_chunks_buyback_and_readonly_quote_rolls_back_every_swap() {
    ext().execute_with(|| {
        open(Side::Short, 1000);
        write(b"test/max_buy", 49_u64);
        let before_market = market(netuid());
        let before_swaps = swap_count();
        let before_position = position();
        let escrow = Lending::position_account(&account(1), netuid());
        let before_cash = tao(&escrow);
        let quote = Lending::quote_close(&account(1), netuid(), false).unwrap();
        assert_eq!(market(netuid()), before_market);
        assert_eq!(swap_count(), before_swaps);
        assert_eq!(position(), before_position);
        assert_eq!(tao(&escrow), before_cash);
        assert_ok!(Lending::close(
            RuntimeOrigin::signed(account(1)),
            netuid(),
            false,
            quote.payment,
            quote.refund
        ));
        assert_eq!(swap_count() - before_swaps, 6);
        assert_eq!(Vaults::<Test>::get(netuid()).unwrap().outstanding_alpha, 0);
    });
}

#[test]
fn failed_six_chunk_close_rolls_back_partial_buybacks_and_interest() {
    ext().execute_with(|| {
        open(Side::Short, 1000);
        System::set_block_number(2);
        write(b"test/max_buy", 1_u64);
        let before_market = market(netuid());
        let before_swaps = swap_count();
        let before_position = position();
        let before_vault = Vaults::<Test>::get(netuid());
        assert!(Lending::quote_close(&account(1), netuid(), false).is_err());
        assert_noop!(
            Lending::close(
                RuntimeOrigin::signed(account(1)),
                netuid(),
                false,
                u64::MAX,
                0
            ),
            Error::<Test>::InsufficientEscrow
        );
        assert_eq!(market(netuid()), before_market);
        assert_eq!(swap_count(), before_swaps);
        assert_eq!(position(), before_position);
        assert_eq!(Vaults::<Test>::get(netuid()), before_vault);
    });
}

#[test]
fn hotkey_swap_updates_saved_sources_without_blocking_nominated_borrowers() {
    ext().execute_with(|| {
        open(Side::Long, 1000);
        let old = account(1);
        let new = account(2);
        assert_ok!(Lending::on_hotkey_swap(&old, &new, Some(99.into())));
        assert_eq!(position().hotkey, old);
        assert_ok!(Lending::on_hotkey_swap(&old, &new, Some(netuid())));
        assert_eq!(position().hotkey, new);
        assert!(!LoanHotkeys::<Test>::contains_key(&old));
        assert_eq!(LoanHotkeys::<Test>::get(&new), 1);
        assert_ok!(Lending::close(
            RuntimeOrigin::signed(old),
            netuid(),
            true,
            u64::MAX,
            0
        ));
        assert!(!LoanHotkeys::<Test>::contains_key(new));
    });
}

#[test]
fn terminal_dust_refund_is_explicit_and_does_not_pin_cleanup() {
    ext().execute_with(|| {
        open(Side::Long, 1000);
        let debt = position().principal;
        assert_ok!(Lending::start_dissolution(netuid()));
        assert!(Lending::settle_shorts(netuid(), &mut meter()));
        write(b"test/ed", 500_u64);
        write(&key(b"test/tao", account(1)), 0_u64);
        let escrow = Lending::position_account(&account(1), netuid());
        mint_tao(&escrow, debt + 1);
        assert_ok!(Lending::on_alpha_redemption(
            netuid(),
            &escrow,
            (debt + 1).into()
        ));
        assert!(Lending::settle_remaining_longs(netuid(), &mut meter()));
        assert_eq!(tao(&account(1)), 0);
        assert_eq!(tao(&escrow), 0);
        assert!(System::events().iter().any(|event| event.event
            == RuntimeEvent::Lending(Event::DustForfeited {
                netuid: netuid(),
                recipient: account(1),
                tao: 1
            })));
        assert_ok!(Lending::finish_dissolution(netuid()));
        assert!(!Vaults::<Test>::contains_key(netuid()));
    });
}

#[test]
fn buyback_quote_charges_interest_before_testing_preserved_balance() {
    ext().execute_with(|| {
        open(Side::Short, 1000);
        System::set_block_number(2);
        let before_position = position();
        let before_vault = Vaults::<Test>::get(netuid());
        let escrow = Lending::position_account(&account(1), netuid());
        let before_cash = tao(&escrow);
        let nominal = Lending::quote_close(&account(1), netuid(), false).unwrap();
        write(b"test/ed", nominal.refund + 1);
        assert!(Lending::quote_close(&account(1), netuid(), false).is_err());
        assert!(
            Lending::close(
                RuntimeOrigin::signed(account(1)),
                netuid(),
                false,
                u64::MAX,
                0
            )
            .is_err()
        );
        assert_eq!(position(), before_position);
        assert_eq!(Vaults::<Test>::get(netuid()), before_vault);
        assert_eq!(tao(&escrow), before_cash);
        write(b"test/ed", nominal.refund);
        assert_eq!(
            Lending::quote_close(&account(1), netuid(), false).unwrap(),
            nominal
        );
        assert_ok!(Lending::close(
            RuntimeOrigin::signed(account(1)),
            netuid(),
            false,
            nominal.payment,
            nominal.refund
        ));
    });
}

#[test]
fn first_sub_existential_coupon_is_explicitly_recycled_without_stalling_close() {
    ext().execute_with(|| {
        open(Side::Short, 1000);
        let vault = Lending::reserve_account(netuid());
        write(&key(b"test/tao", &vault), 0_u64);
        Vaults::<Test>::mutate(netuid(), |value| value.as_mut().unwrap().available_tao = 0);
        write(b"test/ed", 500_u64);
        System::set_block_number(2);
        let quote = Lending::quote_close(&account(1), netuid(), true).unwrap();
        let debt = position().principal;
        assert_ok!(Lending::close(
            RuntimeOrigin::signed(account(1)),
            netuid(),
            true,
            debt,
            quote.refund
        ));
        assert_eq!(tao(&vault), 0);
        assert_eq!(Vaults::<Test>::get(netuid()).unwrap().pending_tao, 0);
        assert!(System::events().iter().any(|event| event.event
            == RuntimeEvent::Lending(Event::DustForfeited {
                netuid: netuid(),
                recipient: vault.clone(),
                tao: 1
            })));
    });
}

#[test]
fn sub_existential_terminal_short_recovery_does_not_pin_an_empty_vault() {
    ext().execute_with(|| {
        open(Side::Short, 1000);
        let debt = position().principal;
        let vault = Lending::reserve_account(netuid());
        write(&key(b"test/tao", &vault), 0_u64);
        Vaults::<Test>::mutate(netuid(), |value| value.as_mut().unwrap().available_tao = 0);
        write(b"test/ed", 500_u64);
        write(&key(b"test/market", netuid()), (1_000_000_u64, 1_u64));
        References::<Test>::mutate(netuid(), |reference| {
            reference.as_mut().unwrap().price = U64F64::from_num(1) / U64F64::from_num(1_000_000);
        });
        assert_ok!(Lending::start_dissolution(netuid()));
        assert!(Lending::settle_shorts(netuid(), &mut meter()));
        assert_eq!(tao(&vault), 0);
        assert_eq!(Vaults::<Test>::get(netuid()).unwrap().lost_alpha, debt);
        assert!(System::events().iter().any(|event| event.event
            == RuntimeEvent::Lending(Event::DustForfeited {
                netuid: netuid(),
                recipient: vault.clone(),
                tao: 1
            })));
    });
}

#[test]
fn multiple_terminal_receipts_retire_the_long_only_after_all_redemptions() {
    ext().execute_with(|| {
        open(Side::Long, 1000);
        let debt = position().principal;
        let owner_before = tao(&account(1));
        let escrow = Lending::position_account(&account(1), netuid());
        assert_ok!(Lending::start_dissolution(netuid()));
        assert!(Lending::settle_shorts(netuid(), &mut meter()));
        for paid in [1, debt + 100] {
            mint_tao(&escrow, paid);
            assert_ok!(Lending::on_alpha_redemption(netuid(), &escrow, paid.into()));
            assert!(Positions::<Test>::contains_key(account(1), netuid()));
            assert_eq!(Vaults::<Test>::get(netuid()).unwrap().outstanding_tao, debt);
            assert_eq!(tao(&account(1)), owner_before);
        }
        assert_eq!(position().proceeds, debt + 101);
        assert!(Lending::settle_remaining_longs(netuid(), &mut meter()));
        assert_eq!(tao(&account(1)), owner_before + 101);
        assert_eq!(tao(&escrow), 0);
        assert_eq!(Vaults::<Test>::get(netuid()).unwrap().available_tao, debt);
        assert_eq!(Vaults::<Test>::get(netuid()).unwrap().lost_tao, 0);
        assert!(!EscrowOwner::<Test>::contains_key(netuid(), &escrow));
    });
}

fn seed_pending_coupon(side: Side, amount: u64) {
    let vault = Lending::reserve_account(netuid());
    let hotkey = Lending::custody_hotkey().unwrap();
    match side {
        Side::Short => mint_tao(&vault, amount),
        Side::Long => mint_alpha(&vault, &hotkey, netuid(), amount),
    }
    Vaults::<Test>::mutate(netuid(), |value| {
        let value = value.as_mut().unwrap();
        match side {
            Side::Short => value.pending_tao += amount,
            Side::Long => value.pending_alpha += amount,
        }
    });
}

fn assert_coupon_inventory_is_backed() {
    let vault = Vaults::<Test>::get(netuid()).unwrap();
    let account = Lending::reserve_account(netuid());
    let hotkey = Lending::custody_hotkey().unwrap();
    assert_eq!(tao(&account), vault.available_tao + vault.pending_tao);
    assert_eq!(
        alpha(&account, &hotkey, netuid()),
        vault.available_alpha + vault.pending_alpha
    );
    assert_eq!(vault.outstanding_alpha, 0);
    assert_eq!(vault.outstanding_tao, 0);
    assert_eq!(vault.lost_alpha, 0);
    assert_eq!(vault.lost_tao, 0);
}

#[test]
fn short_fee_burn_needs_neither_price_reference_nor_amm_execution() {
    for amount in [1_u64, 5_000] {
        ext().execute_with(|| {
            seed_pending_coupon(Side::Short, amount);
            References::<Test>::remove(netuid());
            write(&key(b"test/market", netuid()), (0_u64, 0_u64));
            write(b"test/fail_buy", true);
            write(b"test/fail_sell", true);
            let before = Vaults::<Test>::get(netuid()).unwrap();
            let swaps = swap_count();
            idle(11);
            let after = Vaults::<Test>::get(netuid()).unwrap();
            assert_eq!(after.pending_tao, 0);
            assert_eq!(after.available_tao, before.available_tao);
            assert_eq!(after.available_alpha, before.available_alpha);
            assert_eq!(burned_tao(), amount);
            assert_eq!(swap_count(), swaps);
            assert_eq!(market(netuid()), (0, 0));
            assert!(!References::<Test>::contains_key(netuid()));
            assert_coupon_inventory_is_backed();
            System::assert_last_event(RuntimeEvent::Lending(Event::InterestBurned {
                netuid: netuid(),
                side: Side::Short,
                tao: amount,
            }));
        });
    }
}

#[test]
fn long_fee_burn_failure_rolls_back_sale_and_retries_the_original_alpha() {
    ext().execute_with(|| {
        seed_pending_coupon(Side::Long, 5_000);
        let before = Vaults::<Test>::get(netuid()).unwrap();
        let before_market = market(netuid());
        let before_events = System::events();
        let swaps = swap_count();
        let output = MockPool::sell_quote(netuid(), 5_000).unwrap();
        write(b"test/fail_burn", true);
        idle(11);
        assert_eq!(Vaults::<Test>::get(netuid()), Some(before.clone()));
        assert_eq!(market(netuid()), before_market);
        assert_eq!(swap_count(), swaps);
        assert_eq!(System::events(), before_events);
        assert_eq!(burned_tao(), 0);
        assert_coupon_inventory_is_backed();

        write(b"test/fail_burn", false);
        idle(12);
        let after = Vaults::<Test>::get(netuid()).unwrap();
        assert_eq!(after.pending_alpha, 0);
        assert_eq!(after.pending_tao, 0);
        assert_eq!(after.available_tao, before.available_tao);
        assert_eq!(after.available_alpha, before.available_alpha);
        assert_eq!(burned_tao(), output);
        assert_eq!(swap_count(), swaps + 1);
        assert_coupon_inventory_is_backed();
        System::assert_last_event(RuntimeEvent::Lending(Event::InterestBurned {
            netuid: netuid(),
            side: Side::Long,
            tao: output,
        }));
    });
}

#[test]
fn wallet_repayment_restores_principal_without_burning_it() {
    for side in [Side::Short, Side::Long] {
        ext().execute_with(|| {
            open(side, 1_000);
            let debt = position().principal;
            assert_ok!(Lending::close(
                RuntimeOrigin::signed(account(1)),
                netuid(),
                true,
                debt,
                1_000
            ));
            let vault = Vaults::<Test>::get(netuid()).unwrap();
            assert_eq!(vault.available_tao, 100_000);
            assert_eq!(vault.available_alpha, 100_000);
            assert_eq!(vault.outstanding_tao, 0);
            assert_eq!(vault.outstanding_alpha, 0);
            assert_eq!(vault.pending_tao, 0);
            assert_eq!(vault.pending_alpha, 0);
            assert_eq!(burned_tao(), 0);
            assert_coupon_inventory_is_backed();
            assert!(System::events().iter().all(|event| !matches!(
                event.event,
                RuntimeEvent::Lending(Event::InterestBurned { .. })
            )));
        });
    }
}

#[test]
fn terminal_fees_burn_only_actual_receipts_and_wait_for_every_payout_page() {
    ext().execute_with(|| {
        seed_pending_coupon(Side::Short, 7);
        seed_pending_coupon(Side::Long, 5_000);
        let account = Lending::reserve_account(netuid());
        let hotkey = Lending::custody_hotkey().unwrap();
        let swaps = swap_count();
        assert_ok!(Lending::start_dissolution(netuid()));
        assert!(Lending::settle_shorts(netuid(), &mut meter()));
        assert_eq!(read::<(u64, u64)>(b"test/returned"), (100_000, 100_000));
        assert_eq!(burned_tao(), 7);
        assert_eq!(tao(&account), 0);
        assert_eq!(alpha(&account, &hotkey, netuid()), 5_000);
        assert_eq!(Vaults::<Test>::get(netuid()).unwrap().pending_alpha, 5_000);
        assert!(Lending::finish_dissolution(netuid()).is_err());

        for receipt in [11_u64, 17] {
            mint_tao(&account, receipt);
            assert_ok!(Lending::on_alpha_redemption(
                netuid(),
                &account,
                receipt.into()
            ));
            let vault = Vaults::<Test>::get(netuid()).unwrap();
            assert_eq!(vault.pending_alpha, 5_000);
            assert_eq!(vault.available_tao, 0);
            assert_eq!(tao(&account), 0);
        }
        assert_eq!(burned_tao(), 35);
        assert_eq!(swap_count(), swaps);
        // Model the ordinary payout's obsolete-alpha cleanup, separate from
        // lending's ledger retirement after every funded page has completed.
        write(&key(b"test/alpha", (&account, &hotkey, netuid())), 0_u64);
        assert!(Lending::settle_remaining_longs(netuid(), &mut meter()));
        assert_eq!(Vaults::<Test>::get(netuid()).unwrap().pending_alpha, 0);
        assert_ok!(Lending::finish_dissolution(netuid()));
        assert!(!Vaults::<Test>::contains_key(netuid()));
        assert_eq!(burned_tao(), 35);
    });
}

#[test]
fn terminal_zero_fee_payout_does_not_invent_tao_or_leave_a_stale_ledger() {
    ext().execute_with(|| {
        seed_pending_coupon(Side::Long, 5_000);
        let account = Lending::reserve_account(netuid());
        let hotkey = Lending::custody_hotkey().unwrap();
        assert_ok!(Lending::start_dissolution(netuid()));
        assert!(Lending::settle_shorts(netuid(), &mut meter()));
        assert_ok!(Lending::on_alpha_redemption(
            netuid(),
            &account,
            TaoBalance::ZERO
        ));
        assert_eq!(Vaults::<Test>::get(netuid()).unwrap().pending_alpha, 5_000);
        assert_eq!(burned_tao(), 0);
        write(&key(b"test/alpha", (&account, &hotkey, netuid())), 0_u64);
        assert!(Lending::settle_remaining_longs(netuid(), &mut meter()));
        assert_eq!(Vaults::<Test>::get(netuid()).unwrap().pending_alpha, 0);
        assert_ok!(Lending::finish_dissolution(netuid()));
        assert_eq!(burned_tao(), 0);
        assert_eq!(swap_count(), 0);
    });
}

#[test]
fn terminal_receipt_burn_failure_keeps_real_tao_and_retries_without_retiring_alpha() {
    ext().execute_with(|| {
        seed_pending_coupon(Side::Long, 5_000);
        let account = Lending::reserve_account(netuid());
        assert_ok!(Lending::start_dissolution(netuid()));
        assert!(Lending::settle_shorts(netuid(), &mut meter()));
        mint_tao(&account, 25);
        let before = Vaults::<Test>::get(netuid()).unwrap();
        write(b"test/fail_burn", true);
        assert_noop!(
            Lending::on_alpha_redemption(netuid(), &account, 25.into()),
            DispatchError::Other("burn unavailable")
        );
        assert_eq!(tao(&account), 25);
        assert_eq!(burned_tao(), 0);
        assert_eq!(Vaults::<Test>::get(netuid()), Some(before.clone()));
        write(b"test/fail_burn", false);
        assert_ok!(Lending::on_alpha_redemption(netuid(), &account, 25.into()));
        assert_eq!(tao(&account), 0);
        assert_eq!(burned_tao(), 25);
        assert_eq!(Vaults::<Test>::get(netuid()), Some(before));
    });
}

#[test]
fn failed_terminal_reserve_return_rolls_back_the_prior_short_fee_burn() {
    ext().execute_with(|| {
        seed_pending_coupon(Side::Short, 7);
        seed_pending_coupon(Side::Long, 5_000);
        assert_ok!(Lending::start_dissolution(netuid()));
        let before = Vaults::<Test>::get(netuid()).unwrap();
        let events = System::events();
        write(b"test/fail_transfer", true);
        assert!(!Lending::settle_shorts(netuid(), &mut meter()));
        assert_eq!(burned_tao(), 0);
        assert_eq!(Vaults::<Test>::get(netuid()), Some(before));
        assert!(
            !Dissolutions::<Test>::get(netuid())
                .unwrap()
                .reserves_returned
        );
        assert_eq!(System::events(), events);
        assert_coupon_inventory_is_backed();
        write(b"test/fail_transfer", false);
        assert!(Lending::settle_shorts(netuid(), &mut meter()));
        assert_eq!(burned_tao(), 7);
        assert_eq!(Vaults::<Test>::get(netuid()).unwrap().pending_alpha, 5_000);
    });
}

#[test]
fn adverse_long_fee_sale_holds_alpha_until_ema_bounded_execution_returns() {
    ext().execute_with(|| {
        seed_pending_coupon(Side::Long, 5_000);
        let manipulated = (2_000_000_u64, 1_000_000_u64);
        write(&key(b"test/market", netuid()), manipulated);
        let before = Vaults::<Test>::get(netuid()).unwrap();
        let reference = References::<Test>::get(netuid()).unwrap();
        let swaps = swap_count();
        idle(11);
        assert_eq!(Vaults::<Test>::get(netuid()), Some(before));
        assert_eq!(swap_count(), swaps);
        assert_eq!(market(netuid()), manipulated);
        assert_eq!(References::<Test>::get(netuid()), Some(reference));
        assert_coupon_inventory_is_backed();
        assert_eq!(burned_tao(), 0);

        // Returning to the mature reference admits normal ~1% ending-price
        // depth and the mock's 0.1% fee within the fixed 2% output allowance.
        write(
            &key(b"test/market", netuid()),
            (1_000_000_u64, 1_000_000_u64),
        );
        idle(12);
        let after = Vaults::<Test>::get(netuid()).unwrap();
        assert_eq!(after.pending_tao, 0);
        assert_eq!(after.pending_alpha, 0);
        assert_eq!(swap_count(), swaps + 1);
        assert_eq!(after.available_tao, 100_000);
        assert_eq!(after.available_alpha, 100_000);
        assert!(burned_tao() >= 4_900);
        assert_coupon_inventory_is_backed();
    });
}

#[test]
fn long_fee_conversion_waits_for_a_present_mature_reference() {
    for missing in [false, true] {
        ext().execute_with(|| {
            seed_pending_coupon(Side::Long, 5_000);
            let mut reference = References::<Test>::get(netuid()).unwrap();
            reference.valid_after = 12;
            if missing {
                References::<Test>::remove(netuid());
            } else {
                References::<Test>::insert(netuid(), reference.clone());
            }
            let before = Vaults::<Test>::get(netuid()).unwrap();
            idle(11);
            assert_eq!(Vaults::<Test>::get(netuid()), Some(before));
            assert_eq!(swap_count(), 0);
            assert_eq!(burned_tao(), 0);
            assert_coupon_inventory_is_backed();
            References::<Test>::insert(netuid(), reference);
            idle(12);
            let after = Vaults::<Test>::get(netuid()).unwrap();
            assert_eq!(after.pending_tao, 0);
            assert_eq!(after.pending_alpha, 0);
            assert_eq!(swap_count(), 1);
            assert!(burned_tao() >= 4_900);
            assert_coupon_inventory_is_backed();
        });
    }
}

#[test]
fn long_fee_depth_guard_burns_a_smaller_chunk_and_preserves_the_remainder() {
    ext().execute_with(|| {
        seed_pending_coupon(Side::Long, 50_000);
        let before = Vaults::<Test>::get(netuid()).unwrap();
        idle(11);
        let after = Vaults::<Test>::get(netuid()).unwrap();
        assert_eq!(after.pending_alpha, 37_500);
        assert_eq!(after.available_tao, before.available_tao);
        assert_eq!(after.available_alpha, before.available_alpha);
        let received = burned_tao();
        assert!(
            received >= 12_250,
            "12500 input retains at least 98% EMA output"
        );
        assert_eq!(swap_count(), 1);
        assert_coupon_inventory_is_backed();
    });
}

#[test]
fn long_fee_guard_permits_favorable_prices_and_rejects_zero_output_dust() {
    ext().execute_with(|| {
        seed_pending_coupon(Side::Long, 5_000);
        let favorable = (1_000_000_u64, 2_000_000_u64);
        write(&key(b"test/market", netuid()), favorable);
        idle(11);
        let after = Vaults::<Test>::get(netuid()).unwrap();
        assert_eq!(after.pending_tao, 0);
        assert_eq!(after.pending_alpha, 0);
        assert_eq!(swap_count(), 1);
        assert!(burned_tao() > 5_000);
        assert_coupon_inventory_is_backed();
    });
    ext().execute_with(|| {
        seed_pending_coupon(Side::Long, 1);
        let before = Vaults::<Test>::get(netuid()).unwrap();
        idle(11);
        assert_eq!(Vaults::<Test>::get(netuid()), Some(before));
        assert_eq!(swap_count(), 0);
        assert_eq!(burned_tao(), 0);
        assert_coupon_inventory_is_backed();
    });
}

#[test]
fn coupon_output_budget_includes_fees_and_rounds_only_to_output_atoms() {
    assert_eq!(
        Lending::coupon_minimum_output(100, U64F64::from_num(1)).unwrap(),
        98
    );
    assert_eq!(
        Lending::coupon_minimum_output(50, U64F64::from_num(2)).unwrap(),
        98
    );
}

#[test]
fn unrepresentable_ema_long_fee_valuation_keeps_alpha_pending() {
    let price = U64F64::from_num(u64::MAX);
    assert!(Lending::coupon_minimum_output(100, price).is_err());
    ext().execute_with(|| {
        seed_pending_coupon(Side::Long, 100);
        References::<Test>::mutate(netuid(), |reference| {
            reference.as_mut().unwrap().price = price;
        });
        let before = Vaults::<Test>::get(netuid()).unwrap();
        idle(11);
        assert_eq!(Vaults::<Test>::get(netuid()), Some(before));
        assert_eq!(swap_count(), 0);
        assert_eq!(burned_tao(), 0);
        assert_coupon_inventory_is_backed();
    });
}
