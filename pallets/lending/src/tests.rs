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
    fn fast_alpha_price(_: NetUid) -> Option<U64F64> {
        read(b"test/fast_price")
    }
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
    fn redemption_alpha_supply(_: NetUid) -> Result<u128, DispatchError> {
        let supply = read::<Option<u128>>(b"test/redemption_supply").unwrap_or(1_000);
        ensure!(supply > 0, DispatchError::Other("redemption unavailable"));
        Ok(supply)
    }
    fn alpha_loan_redemption_basis(
        _: NetUid,
        remaining_unloaned_alpha: AlphaBalance,
    ) -> Result<(TaoBalance, u128), DispatchError> {
        ensure!(
            !read::<bool>(b"test/fail_alpha_basis"),
            DispatchError::Other("redemption unavailable")
        );
        let (pot, supply, includes_vault) = read::<Option<(u64, u128, bool)>>(
            b"test/alpha_redemption_basis",
        )
        .unwrap_or((0, u128::from(u64::MAX), false));
        let supply = if includes_vault {
            supply
                .saturating_add(u128::from(remaining_unloaned_alpha.to_u64()))
                .min(u128::from(u64::MAX))
        } else {
            supply
        };
        Ok((pot.into(), supply))
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

#[test]
fn repeated_growth_preserves_fractional_coupons_and_original_due_on_both_sides() {
    for side in [Side::Short, Side::Long] {
        ext().execute_with(|| {
            open(side, 40);
            let due = position().due;
            for now in 2..9 {
                System::set_block_number(now);
                if now == 4 {
                    References::<Test>::mutate(netuid(), |reference| {
                        reference.as_mut().unwrap().price = if side == Side::Short {
                            U64F64::from_num(0.5)
                        } else {
                            U64F64::from_num(2)
                        };
                    });
                }
                let before = position();
                let numerator = u128::from(before.annual_interest)
                    * u128::from(now - before.last_accrued)
                    + u128::from(before.interest_remainder);
                let quote =
                    Lending::quote_open_for(&account(1), netuid(), side, 80, &account(1)).unwrap();
                open(side, 80);
                let after = position();
                assert_eq!(after.principal, before.principal + quote.principal);
                assert_eq!(
                    after.annual_interest,
                    before.annual_interest + quote.annual_interest
                );
                assert_eq!(
                    after.collateral,
                    before.collateral + 80 - (numerator / 520) as u64
                );
                assert_eq!(after.interest_remainder, (numerator % 520) as u64);
                assert_eq!(after.due, due);
                assert_eq!(PositionCount::<Test>::get(netuid()), 1);
                assert_eq!(TotalPositions::<Test>::get(), 1);
                assert_eq!(LoanHotkeys::<Test>::get(account(1)), 1);
                assert_eq!(Due::<Test>::iter().count(), 1);
            }
        });
    }
}

#[test]
fn added_collateral_cannot_revive_a_coupon_exhausted_position() {
    for side in [Side::Short, Side::Long] {
        ext().execute_with(|| {
            open(side, 40);
            System::set_block_number(1 + 520 * 10);
            assert_eq!(
                Lending::quote_open_for(&account(1), netuid(), side, 1000, &account(1)),
                Err(Error::<Test>::InsufficientEscrow.into())
            );
            assert_noop!(
                Lending::open(
                    RuntimeOrigin::signed(account(1)),
                    netuid(),
                    side,
                    1000,
                    account(1),
                    1,
                    0,
                ),
                Error::<Test>::InsufficientEscrow
            );
        });
    }
}

#[test]
fn growth_quotes_reject_collateral_and_coupon_overflow_before_custody() {
    for field in [0, 1] {
        ext().execute_with(|| {
            let side = if field == 0 { Side::Long } else { Side::Short };
            open(side, 1000);
            Positions::<Test>::mutate(account(1), netuid(), |position| {
                let position = position.as_mut().unwrap();
                match field {
                    0 => {
                        position.collateral = u64::MAX;
                        position.annual_interest = 0;
                        position.interest_remainder = 1;
                    }
                    _ => position.annual_interest = u64::MAX,
                }
            });
            let added = if field == 0 { 1 } else { 1000 };
            assert_eq!(
                Lending::quote_open_for(&account(1), netuid(), side, added, &account(1)),
                Err(Error::<Test>::Arithmetic.into())
            );
            assert_noop!(
                Lending::open(
                    RuntimeOrigin::signed(account(1)),
                    netuid(),
                    side,
                    added,
                    account(1),
                    1,
                    0,
                ),
                Error::<Test>::Arithmetic
            );
        });
    }
}

#[test]
fn grown_debt_uses_exact_funded_boundary_and_preserves_each_other_loans_coverage() {
    ext().execute_with(|| {
        write(b"test/redemption_supply", Some(2000_u128));
        indexed_long(1, 50, 400);
        let delta =
            Lending::funded_long_limit_for(netuid(), 800, 1000, 50, Some(&account(1))).unwrap();
        assert_eq!(delta, 45);
        assert!(4 * (50 + u128::from(delta)) * 2000 <= 800 * (1000 - u128::from(delta)));
        assert!(4 * (50 + u128::from(delta + 1)) * 2000 > 800 * (1000 - u128::from(delta + 1)));
        indexed_long(2, 100, 208);
        let limited =
            Lending::funded_long_limit_for(netuid(), 800, 1000, 50, Some(&account(1))).unwrap();
        assert_eq!(limited, 38);
        assert!(208 * (1000 - u128::from(limited)) >= 100 * 2000);
        assert!(208 * (1000 - u128::from(limited + 1)) < 100 * 2000);
    });
}

#[test]
fn global_borrowing_cap_counts_additional_debt_once_and_rejects_later_growth() {
    ext().execute_with(|| {
        open(Side::Long, 40_000);
        let first = position().principal;
        assert!(first > 9000 && first < 10_000);
        let quote =
            Lending::quote_open_for(&account(1), netuid(), Side::Long, 400, &account(1)).unwrap();
        open(Side::Long, 400);
        let vault = Vaults::<Test>::get(netuid()).unwrap();
        assert_eq!(vault.outstanding_tao, first + quote.principal);
        assert!(vault.outstanding_tao <= 10_000);
        assert_noop!(
            Lending::open(
                RuntimeOrigin::signed(account(1)),
                netuid(),
                Side::Long,
                400,
                account(1),
                1,
                0,
            ),
            Error::<Test>::BorrowingLimit
        );
    });
}
fn idle(now: u64) {
    System::set_block_number(now);
    Lending::on_idle(now, Weight::MAX);
}
fn meter() -> WeightMeter {
    WeightMeter::with_limit(Weight::MAX)
}

fn indexed_long(owner: u8, principal: u64, collateral: u64) {
    let owner = account(owner);
    Positions::<Test>::insert(
        &owner,
        netuid(),
        Position {
            side: Side::Long,
            hotkey: owner.clone(),
            principal,
            collateral,
            proceeds: 0,
            annual_interest: 0,
            last_accrued: 1,
            interest_remainder: 0,
            due: 11,
        },
    );
    OpenByNetuid::<Test>::insert(netuid(), &owner, ());
}

#[test]
fn alpha_opening_rejects_immediate_legacy_redemption_without_guaranteed_claims() {
    for supply in [0_u128, 1] {
        ext().execute_with(|| {
            // Unloaned legacy pool alpha does not share the ordinary payout.
            // A free borrowed holder must not obtain this large pot for 1,000 TAO.
            write(
                b"test/alpha_redemption_basis",
                Some((100_000_u64, supply, false)),
            );
            let before = sp_io::storage::root(sp_runtime::StateVersion::V1);
            assert_eq!(
                Lending::quote_open(netuid(), Side::Short, 1000),
                Err(Error::<Test>::InsufficientRedemptionBacking.into())
            );
            assert_noop!(
                Lending::open(
                    RuntimeOrigin::signed(account(1)),
                    netuid(),
                    Side::Short,
                    1000,
                    account(1),
                    1,
                    0
                ),
                Error::<Test>::InsufficientRedemptionBacking
            );
            assert_eq!(sp_io::storage::root(sp_runtime::StateVersion::V1), before);
            // This alpha-side refusal leaves ordinary TAO borrowing available.
            assert!(Lending::quote_open(netuid(), Side::Long, 1000).is_ok());
        });
    }
}

#[test]
fn alpha_funded_cap_covers_quarter_collateral_even_under_maximum_split_row_dust() {
    ext().execute_with(|| {
        write(
            b"test/alpha_redemption_basis",
            Some((100_000_u64, 100_000_u128, false)),
        );
        let quote = Lending::quote_open(netuid(), Side::Short, 1000).unwrap();
        assert_eq!(quote.principal, 83);
        // Actual terminal pot cannot exceed 200,000; actual eligible claims
        // cannot be below 100,000. Splitting every borrowed atom still fits.
        let worst =
            Lending::funded_alpha_value(quote.principal, (200_000, 100_000, u64::MAX)).unwrap();
        assert_eq!(worst, 249);
        assert!(worst <= 250);
        assert!(
            Lending::funded_alpha_value(quote.principal + 1, (200_000, 100_000, u64::MAX)).unwrap()
                > 250
        );
        open(Side::Short, 1000);
        assert_eq!(position().annual_interest, 83);
        assert_eq!(position().proceeds, 0);
        assert_eq!(swap_count(), 0);
        assert_ok!(Lending::start_dissolution(netuid()));
        assert!(Lending::settle_shorts(netuid(), &mut meter()));
        assert_ok!(Lending::freeze_redemption_basis(
            netuid(),
            200_000.into(),
            100_000,
            u64::MAX
        ));
        assert!(Lending::settle_remaining_longs(netuid(), &mut meter()));
        assert_eq!(Vaults::<Test>::get(netuid()).unwrap().available_tao, 249);
        assert_eq!(Vaults::<Test>::get(netuid()).unwrap().lost_alpha, 0);
        assert_eq!(tao(&account(1)), 999_751);
    });
}

#[test]
fn modern_alpha_cap_values_unloaned_inventory_after_the_proposed_withdrawal() {
    ext().execute_with(|| {
        write(
            b"test/alpha_redemption_basis",
            Some((1_000_000_u64, 1_000_000_u128, true)),
        );
        let quote = Lending::quote_open(netuid(), Side::Short, 1000).unwrap();
        assert_eq!(quote.principal, 124);
        let vault = Vaults::<Test>::get(netuid()).unwrap();
        let actual_lower = 1_000_000 + u128::from(vault.available_alpha - quote.principal);
        let worst =
            Lending::funded_alpha_value(quote.principal, (1_100_000, actual_lower, u64::MAX))
                .unwrap();
        assert!(worst <= 250);
        assert!(
            Lending::funded_alpha_value(
                quote.principal + 1,
                (1_100_000, actual_lower - 1, u64::MAX)
            )
            .unwrap()
                > 250
        );
        open(Side::Short, 1000);
        assert_eq!(
            Vaults::<Test>::get(netuid()).unwrap().available_alpha,
            100_000 - 124
        );
    });
}

#[test]
fn alpha_growth_checks_combined_debt_and_cannot_add_funds_to_an_undercovered_mark() {
    ext().execute_with(|| {
        write(
            b"test/alpha_redemption_basis",
            Some((100_000_u64, 100_000_u128, false)),
        );
        open(Side::Short, 1000);
        let original = position();
        System::set_block_number(2);
        let quote =
            Lending::quote_open_for(&account(1), netuid(), Side::Short, 1000, &account(1)).unwrap();
        assert_eq!(quote.principal, 83);
        open(Side::Short, 1000);
        let grown = position();
        assert_eq!(grown.principal, 166);
        assert_eq!(
            grown.annual_interest,
            original.annual_interest + quote.annual_interest
        );
        assert_eq!(grown.interest_remainder, 83);
        assert_eq!(grown.due, original.due);
        assert!(
            Lending::funded_alpha_value(grown.principal, (200_000, 100_000, u64::MAX)).unwrap()
                <= 499
        );
        write(
            b"test/alpha_redemption_basis",
            Some((100_000_u64, 1_u128, false)),
        );
        let before = sp_io::storage::root(sp_runtime::StateVersion::V1);
        assert_noop!(
            Lending::open(
                RuntimeOrigin::signed(account(1)),
                netuid(),
                Side::Short,
                1000,
                account(1),
                1,
                0
            ),
            Error::<Test>::InsufficientRedemptionBacking
        );
        assert_eq!(sp_io::storage::root(sp_runtime::StateVersion::V1), before);
        assert_eq!(position(), grown);
    });
}

#[test]
fn alpha_admission_preserves_each_other_shorts_rounded_coupon_coverage() {
    ext().execute_with(|| {
        write(
            b"test/alpha_redemption_basis",
            Some((100_000_u64, 100_000_u128, false)),
        );
        indexed_long(2, 100, 300);
        Positions::<Test>::mutate(account(2), netuid(), |state| {
            state.as_mut().unwrap().side = Side::Short
        });
        assert_eq!(
            Lending::quote_open(netuid(), Side::Short, 1000)
                .unwrap()
                .principal,
            83
        );
        Positions::<Test>::mutate(account(2), netuid(), |state| {
            state.as_mut().unwrap().annual_interest = 1
        });
        System::set_block_number(2);
        assert_eq!(
            Lending::quote_open(netuid(), Side::Short, 1000),
            Err(Error::<Test>::InsufficientRedemptionBacking.into())
        );
        let before = sp_io::storage::root(sp_runtime::StateVersion::V1);
        assert_noop!(
            Lending::open(
                RuntimeOrigin::signed(account(1)),
                netuid(),
                Side::Short,
                1000,
                account(1),
                1,
                0
            ),
            Error::<Test>::InsufficientRedemptionBacking
        );
        assert_eq!(sp_io::storage::root(sp_runtime::StateVersion::V1), before);
        assert_eq!(
            Positions::<Test>::get(account(2), netuid())
                .unwrap()
                .collateral,
            300
        );
    });
}

#[test]
fn alpha_funded_scan_is_bounded_and_does_not_pool_collateral_or_expected_recoveries() {
    ext().execute_with(|| {
        write(
            b"test/alpha_redemption_basis",
            Some((100_000_u64, 100_000_u128, false)),
        );
        for n in 2..=5 {
            indexed_long(n, 1, 1000);
            Positions::<Test>::mutate(account(n), netuid(), |state| {
                state.as_mut().unwrap().side = Side::Short
            });
        }
        assert_eq!(
            Lending::quote_open(netuid(), Side::Short, 1000)
                .unwrap()
                .principal,
            83
        );
        indexed_long(6, 1, 1000);
        assert_eq!(
            Lending::quote_open(netuid(), Side::Short, 1000),
            Err(Error::<Test>::TooManyPositions.into())
        );
    });
    ext().execute_with(|| {
        write(
            b"test/alpha_redemption_basis",
            Some((100_000_u64, 100_000_u128, false)),
        );
        let quote = Lending::quote_open(netuid(), Side::Short, 1000).unwrap();
        Vaults::<Test>::mutate(netuid(), |value| {
            let value = value.as_mut().unwrap();
            value.pending_tao = 500_000;
            value.pending_alpha = 500_000;
            value.outstanding_tao = 500_000;
        });
        assert_eq!(Lending::quote_open(netuid(), Side::Short, 1000), Ok(quote));
        indexed_long(2, 100, 299);
        Positions::<Test>::mutate(account(2), netuid(), |state| {
            let state = state.as_mut().unwrap();
            state.side = Side::Short;
            state.proceeds = 500_000;
        });
        indexed_long(3, 1, 1_000_000);
        Positions::<Test>::mutate(account(3), netuid(), |state| {
            state.as_mut().unwrap().side = Side::Short
        });
        assert_eq!(
            Lending::quote_open(netuid(), Side::Short, 1000),
            Err(Error::<Test>::InsufficientRedemptionBacking.into())
        );
    });
}

#[test]
fn alpha_funded_arithmetic_caps_cash_domain_and_fails_closed_on_bad_basis() {
    ext().execute_with(|| {
        let vault = Vault {
            available_tao: u64::MAX,
            available_alpha: u64::MAX,
            ..Vault::default()
        };
        write(
            b"test/alpha_redemption_basis",
            Some((u64::MAX, u128::from(u64::MAX), false)),
        );
        assert_eq!(
            Lending::funded_alpha_limit_for(netuid(), u64::MAX, &vault, 0, u64::MAX, None).unwrap(),
            (u64::MAX / 4) / 2
        );
        write(
            b"test/alpha_redemption_basis",
            Some((u64::MAX, u128::MAX, false)),
        );
        assert_eq!(
            Lending::funded_alpha_limit_for(netuid(), u64::MAX, &vault, 0, u64::MAX, None),
            Err(Error::<Test>::Arithmetic.into())
        );
        write(b"test/fail_alpha_basis", true);
        assert_eq!(
            Lending::quote_open(netuid(), Side::Short, 1000),
            Err(Error::<Test>::RedemptionUnavailable.into())
        );
    });
}

#[test]
fn funded_long_limit_is_maximal_at_quarter_of_post_withdrawal_redemption() {
    ext().execute_with(|| {
        for (collateral, backing, supply) in [
            (4_u64, 100_000_u64, 200_001_u128),
            (1_000, 100_000, 200_001),
            (7_777, 98_765, 123_457),
            (u64::MAX, u64::MAX, u128::from(u64::MAX)),
        ] {
            write(b"test/redemption_supply", Some(supply));
            let expected = u128::from(collateral) * u128::from(backing)
                / (4 * supply + u128::from(collateral));
            if expected == 0 {
                assert_eq!(
                    Lending::funded_long_limit(netuid(), collateral, backing),
                    Err(Error::<Test>::InsufficientRedemptionBacking.into())
                );
                continue;
            }
            let loan = Lending::funded_long_limit(netuid(), collateral, backing).unwrap();
            assert_eq!(u128::from(loan), expected);
            assert!(
                4 * u128::from(loan) * supply
                    <= u128::from(collateral) * u128::from(backing - loan)
            );
            let larger = loan + 1;
            assert!(
                4 * u128::from(larger) * supply
                    > u128::from(collateral) * u128::from(backing - larger)
            );
        }
    });
}

#[test]
fn long_quote_and_open_use_redemption_backing_after_their_own_withdrawal() {
    ext().execute_with(|| {
        write(b"test/redemption_supply", Some(200_001_u128));
        let quote = Lending::quote_open(netuid(), Side::Long, 1_000).unwrap();
        assert_eq!(quote.principal, 124);
        assert!(quote.principal < MockPool::sell_quote(netuid(), 250).unwrap());
        let before = tao(&account(1));
        open(Side::Long, 1_000);
        let vault = Vaults::<Test>::get(netuid()).unwrap();
        assert_eq!(position().principal, quote.principal);
        assert_eq!(tao(&account(1)) - before, quote.principal);
        assert_eq!(vault.available_tao, 100_000 - quote.principal);
        assert_eq!(vault.outstanding_tao, quote.principal);
        assert!(
            4 * u128::from(quote.principal) * 200_001 <= 1_000 * u128::from(vault.available_tao)
        );
        assert_eq!(swap_count(), 0);
    });
}

#[test]
fn maintenance_counts_rounded_up_interest_before_the_weekly_collection() {
    ext().execute_with(|| {
        write(b"test/redemption_supply", Some(200_000_u128));
        indexed_long(2, 1_000, 2_002);
        Positions::<Test>::mutate(account(2), netuid(), |p| {
            p.as_mut().unwrap().annual_interest = 1;
        });
        assert_eq!(
            Lending::quote_open(netuid(), Side::Long, 1_000)
                .unwrap()
                .principal,
            99
        );
        System::set_block_number(2);
        let old = Positions::<Test>::get(account(2), netuid()).unwrap();
        assert!(System::block_number() < old.due);
        let quote = Lending::quote_open(netuid(), Side::Long, 1_000).unwrap();
        assert_eq!(quote.principal, 49);
        assert_eq!(Positions::<Test>::get(account(2), netuid()).unwrap(), old);
        open(Side::Long, 1_000);
        let backing = Vaults::<Test>::get(netuid()).unwrap().available_tao;
        assert!(
            u128::from(old.principal) * 200_000
                <= u128::from(old.collateral - 1) * u128::from(backing)
        );
        assert!(
            u128::from(old.principal) * 200_000
                > u128::from(old.collateral - 1) * u128::from(backing - 1)
        );
        assert_eq!(burned_tao(), 0);
    });
}

#[test]
fn prior_fractional_interest_is_part_of_redemption_maintenance() {
    ext().execute_with(|| {
        write(b"test/redemption_supply", Some(200_000_u128));
        indexed_long(2, 1_000, 2_002);
        Positions::<Test>::mutate(account(2), netuid(), |p| {
            p.as_mut().unwrap().interest_remainder = 1;
        });
        assert_eq!(
            Lending::quote_open(netuid(), Side::Long, 1_000)
                .unwrap()
                .principal,
            49
        );
    });
}

#[test]
fn each_existing_loan_needs_coverage_without_using_another_borrowers_surplus() {
    ext().execute_with(|| {
        write(b"test/redemption_supply", Some(200_000_u128));
        indexed_long(2, 1_000, 1_000);
        indexed_long(3, 1, 100_000);
        // Pooling these claims would appear safe; the first borrower is not covered.
        let first = Positions::<Test>::get(account(2), netuid()).unwrap();
        let second = Positions::<Test>::get(account(3), netuid()).unwrap();
        let debt = u128::from(first.principal) + u128::from(second.principal);
        let collateral = u128::from(first.collateral) + u128::from(second.collateral);
        let backing = u128::from(Vaults::<Test>::get(netuid()).unwrap().available_tao);
        assert!(debt * 200_000 <= collateral * backing);
        assert_eq!(
            Lending::quote_open(netuid(), Side::Long, 1_000),
            Err(Error::<Test>::InsufficientRedemptionBacking.into())
        );
        let vault = Vaults::<Test>::get(netuid()).unwrap();
        let owner_tao = tao(&account(1));
        let owner_alpha = alpha(&account(1), &account(1), netuid());
        assert_noop!(
            Lending::open(
                RuntimeOrigin::signed(account(1)),
                netuid(),
                Side::Long,
                1_000,
                account(1),
                1,
                0,
            ),
            Error::<Test>::InsufficientRedemptionBacking
        );
        assert_eq!(Vaults::<Test>::get(netuid()).unwrap(), vault);
        assert_eq!(tao(&account(1)), owner_tao);
        assert_eq!(alpha(&account(1), &account(1), netuid()), owner_alpha);
        assert!(Positions::<Test>::get(account(1), netuid()).is_none());
        assert_eq!(PositionCount::<Test>::get(netuid()), 0);
    });
}

#[test]
fn coupons_outstanding_debt_and_anticipated_recoveries_do_not_fund_new_loans() {
    ext().execute_with(|| {
        write(b"test/redemption_supply", Some(200_001_u128));
        let quote = Lending::quote_open(netuid(), Side::Long, 1_000).unwrap();
        indexed_long(2, 9_000, 100_000);
        Positions::<Test>::mutate(account(2), netuid(), |p| {
            p.as_mut().unwrap().proceeds = 500_000;
        });
        Vaults::<Test>::mutate(netuid(), |vault| {
            let vault = vault.as_mut().unwrap();
            vault.outstanding_tao = 9_000;
            vault.outstanding_alpha = 9_000;
            vault.pending_tao = 500_000;
            vault.pending_alpha = 500_000;
        });
        mint_tao(&Lending::reserve_account(netuid()), 500_000);
        mint_tao(&Lending::recovery_account(), 500_000);
        write(
            &key(b"test/market", netuid()),
            (1_000_000_u64, 100_000_000_u64),
        );
        assert_eq!(Lending::quote_open(netuid(), Side::Long, 1_000), Ok(quote));
        open(Side::Long, 1_000);
        assert_eq!(position().principal, 124);
    });
}

#[test]
fn missing_redemption_or_overflow_fail_closed_without_blocking_shorts() {
    ext().execute_with(|| {
        let short = Lending::quote_open(netuid(), Side::Short, 1_000).unwrap();
        write(b"test/redemption_supply", Some(0_u128));
        assert_noop!(
            Lending::open(
                RuntimeOrigin::signed(account(1)),
                netuid(),
                Side::Long,
                1_000,
                account(1),
                1,
                0,
            ),
            Error::<Test>::RedemptionUnavailable
        );
        assert_eq!(Lending::quote_open(netuid(), Side::Short, 1_000), Ok(short));
        open(Side::Short, 1_000);
    });
    ext().execute_with(|| {
        write(b"test/redemption_supply", Some(u128::MAX));
        assert_noop!(
            Lending::open(
                RuntimeOrigin::signed(account(1)),
                netuid(),
                Side::Long,
                1_000,
                account(1),
                1,
                0,
            ),
            Error::<Test>::Arithmetic
        );
        write(b"test/redemption_supply", Some(u128::MAX / 8));
        indexed_long(2, 9, 1_000);
        assert_eq!(
            Lending::funded_long_limit(netuid(), 1_000, 100_000),
            Err(Error::<Test>::Arithmetic.into())
        );
    });
}

#[test]
fn redemption_maintenance_scan_handles_the_bound_and_rejects_extra_entries() {
    ext().execute_with(|| {
        write(b"test/redemption_supply", Some(200_000_u128));
        for owner in 2..=5 {
            indexed_long(owner, 1_000, 2_002);
        }
        assert_eq!(Lending::funded_long_limit(netuid(), 1_000, 100_000), Ok(99));
        indexed_long(6, 1, 100_000);
        assert_eq!(
            Lending::funded_long_limit(netuid(), 1_000, 100_000),
            Err(Error::<Test>::TooManyPositions.into())
        );
    });
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
fn short_alpha_is_delivered_freely_and_wallet_repayment_restores_fixed_debt() {
    ext().execute_with(|| {
        let before = tao(&account(1));
        let before_alpha = alpha(&account(1), &account(1), netuid());
        let before_market = market(netuid());
        open(Side::Short, 1000);
        let p = position();
        let escrow = Lending::position_account(&account(1), netuid());
        assert_eq!(p.principal, MockPool::buy_quote(netuid(), 250).unwrap());
        assert_eq!(p.annual_interest, p.principal);
        assert_eq!(p.proceeds, 0);
        assert_eq!(tao(&account(1)), before - 1000);
        assert_eq!(
            alpha(&account(1), &account(1), netuid()),
            before_alpha + p.principal
        );
        assert_eq!(tao(&escrow), 1000);
        assert_eq!(market(netuid()), before_market);
        assert_eq!(swap_count(), 0);
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
        assert_eq!(alpha(&account(1), &account(1), netuid()), before_alpha);
        assert_eq!(tao(&account(1)), before);
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
fn borrowed_alpha_can_move_and_fixed_debt_can_be_repaid_from_other_alpha() {
    ext().execute_with(|| {
        open(Side::Short, 1000);
        let debt = position().principal;
        assert_ok!(MockPool::transfer_staked_alpha(
            &account(1),
            &account(1),
            &account(2),
            &account(3),
            netuid(),
            debt.into(),
            false,
            false,
        ));
        assert_eq!(alpha(&account(2), &account(3), netuid()), debt);
        assert_eq!(position().principal, debt);
        assert_eq!(position().proceeds, 0);
        // The fixed debt does not follow the recipient: the original borrower
        // can repay it from any alpha now available on their saved hotkey.
        assert_ok!(Lending::close(
            RuntimeOrigin::signed(account(1)),
            netuid(),
            true,
            debt,
            1000,
        ));
        assert_eq!(alpha(&account(2), &account(3), netuid()), debt);
        assert_eq!(
            Vaults::<Test>::get(netuid()).unwrap().available_alpha,
            100_000
        );
        assert_eq!(Vaults::<Test>::get(netuid()).unwrap().outstanding_alpha, 0);
    });
}

#[test]
fn borrower_can_sell_free_alpha_and_pledge_the_free_proceeds_to_grow() {
    ext().execute_with(|| {
        open(Side::Short, 1000);
        let original = position();
        let proceeds = MockPool::sell_alpha(
            &account(1),
            &account(1),
            netuid(),
            original.principal.into(),
            u64::MAX.into(),
            false,
        )
        .unwrap()
        .to_u64();
        assert!(proceeds > 0);
        let before = tao(&account(1));
        let quote =
            Lending::quote_open_for(&account(1), netuid(), Side::Short, proceeds, &account(1))
                .unwrap();
        open(Side::Short, proceeds);
        assert_eq!(tao(&account(1)), before - proceeds);
        assert_eq!(position().principal, original.principal + quote.principal);
        assert_eq!(position().collateral, 1000 + proceeds);
        assert_eq!(position().proceeds, 0);
        assert_eq!(position().due, original.due);
        assert_eq!(TotalPositions::<Test>::get(), 1);
        assert_eq!(swap_count(), 1);
    });
}

#[test]
fn short_opening_uses_both_purchase_depth_and_reference_value_without_a_swap() {
    for (alpha_reserve, tao_reserve, reference, expected) in [
        (1_000_000_u64, 100_000_000_u64, 1.0, 2_u64),
        (100_000_000, 1_000_000, 1.0, 249),
        (1_000_000, 1_000_000, 0.5, 249),
    ] {
        ext().execute_with(|| {
            write(&key(b"test/market", netuid()), (alpha_reserve, tao_reserve));
            References::<Test>::mutate(netuid(), |state| {
                state.as_mut().unwrap().price = U64F64::from_num(reference);
            });
            let quote = Lending::quote_open(netuid(), Side::Short, 1000).unwrap();
            assert_eq!(quote.principal, expected);
            assert!(quote.principal <= MockPool::buy_quote(netuid(), 250).unwrap());
            assert!(quote.opening_value <= 250);
            assert_eq!(quote.annual_interest, quote.opening_value);
            assert_noop!(
                Lending::open(
                    RuntimeOrigin::signed(account(1)),
                    netuid(),
                    Side::Short,
                    1000,
                    account(1),
                    1,
                    quote.opening_value + 1,
                ),
                Error::<Test>::BelowMinimumProceeds
            );
            open(Side::Short, 1000);
            assert_eq!(position().principal, quote.principal);
            assert_eq!(position().annual_interest, quote.opening_value);
            assert_eq!(position().proceeds, 0);
            assert_eq!(market(netuid()), (alpha_reserve, tao_reserve));
            assert_eq!(swap_count(), 0);
        });
    }
}

#[test]
fn freely_transferred_alpha_is_not_reclaimed_when_interest_forfeits_the_loan() {
    ext().execute_with(|| {
        open(Side::Short, 1000);
        let debt = position().principal;
        assert_ok!(MockPool::transfer_staked_alpha(
            &account(1),
            &account(1),
            &account(2),
            &account(2),
            netuid(),
            debt.into(),
            false,
            false,
        ));
        write(b"test/fail_burn", true);
        write(b"test/fail_buy", true);
        write(b"test/fail_sell", true);
        idle(1 + 520 * 5);
        assert!(!Positions::<Test>::contains_key(account(1), netuid()));
        assert_eq!(alpha(&account(2), &account(2), netuid()), debt);
        let vault = Vaults::<Test>::get(netuid()).unwrap();
        assert_eq!(vault.lost_alpha, debt);
        assert_eq!(vault.outstanding_alpha, 0);
        assert_eq!(vault.available_alpha, 100_000 - debt);
        assert_eq!(vault.available_tao, 100_000);
        assert_eq!(vault.pending_tao, 1000);
        assert_eq!(swap_count(), 0);
    });
}

#[test]
fn optional_short_buyback_cannot_spend_cash_outside_remaining_collateral() {
    ext().execute_with(|| {
        open(Side::Short, 1000);
        let debt = position().principal;
        let escrow = Lending::position_account(&account(1), netuid());
        // Legacy cash proceeds are still refundable, but buyback has only the
        // remaining collateral budget. New free-alpha loans create no proceeds.
        Positions::<Test>::mutate(account(1), netuid(), |state| {
            let state = state.as_mut().unwrap();
            state.collateral = 100;
            state.proceeds = 1000;
        });
        write(&key(b"test/tao", &escrow), 1100_u64);
        let before = position();
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
        assert_eq!(position(), before);
        assert_eq!(tao(&escrow), 1100);
        assert_eq!(swap_count(), 0);
        assert_ok!(Lending::close(
            RuntimeOrigin::signed(account(1)),
            netuid(),
            true,
            debt,
            1100
        ));
        assert_eq!(tao(&escrow), 0);
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
            MockPool::buy_quote(netuid(), 1000).unwrap()
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
        assert_eq!(vault.outstanding_alpha, position().principal);
        assert!(vault.outstanding_alpha > 9800 && vault.outstanding_alpha < 10_000);
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
        assert!(Positions::<Test>::get(account(1), netuid()).is_some());
        let charged = (p.annual_interest * 5).div_ceil(520);
        assert_eq!(burned_tao(), charged);
        assert_eq!(tao(&account(1)), 999_000);
        assert_ok!(Lending::freeze_redemption_basis(
            netuid(),
            0.into(),
            1000,
            1
        ));
        assert!(Lending::settle_remaining_longs(netuid(), &mut meter()));
        assert!(Positions::<Test>::get(account(1), netuid()).is_none());
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
        write(b"test/fast_price", Some(U64F64::from_num(10)));
        assert_ok!(Lending::start_dissolution(netuid()));
        assert!(Lending::settle_shorts(netuid(), &mut meter()));
        assert_ok!(Lending::freeze_redemption_basis(
            netuid(),
            0.into(),
            1000,
            1
        ));
        assert!(Lending::settle_remaining_longs(netuid(), &mut meter()));
        let v = Vaults::<Test>::get(netuid()).unwrap();
        assert!(v.lost_alpha > 0);
        assert_eq!(v.outstanding_alpha, 0);
        assert_eq!(tao(&account(1)), 999_000);
    });
}

#[test]
fn deregistration_marks_higher_ema_once_and_ignores_the_trigger_spot_price() {
    for (fast, expected) in [(None, 2_u64), (Some(1_u64), 2), (Some(3), 3)] {
        ext().execute_with(|| {
            open(Side::Short, 1000);
            References::<Test>::mutate(netuid(), |state| {
                state.as_mut().unwrap().price = U64F64::from_num(2);
            });
            write(b"test/fast_price", fast.map(U64F64::from_num));
            write(&key(b"test/market", netuid()), (1_u64, u64::MAX));
            System::set_block_number(6);
            assert_ok!(Lending::start_dissolution(netuid()));
            assert_eq!(
                Dissolutions::<Test>::get(netuid()).unwrap().price,
                U64F64::from_num(expected)
            );
            write(b"test/fast_price", Some(U64F64::from_num(100)));
            System::set_block_number(1000);
            Lending::on_finalize(1000);
            assert_eq!(
                Dissolutions::<Test>::get(netuid()).unwrap().price,
                U64F64::from_num(expected)
            );
            assert_eq!(Dissolutions::<Test>::get(netuid()).unwrap().frozen_at, 6);
            assert_noop!(
                Lending::start_dissolution(netuid()),
                Error::<Test>::AlreadyDissolving
            );
        });
    }
}

#[test]
fn terminal_short_waits_for_fixed_basis_and_recovers_outside_ordinary_redemption() {
    ext().execute_with(|| {
        open(Side::Short, 1000);
        let debt = position().principal;
        let user_alpha = alpha(&account(1), &account(1), netuid());
        assert_ok!(Lending::start_dissolution(netuid()));
        assert_noop!(
            Lending::freeze_redemption_basis(netuid(), 3000.into(), 1000, 4),
            Error::<Test>::SubnetUnavailable
        );
        assert!(Lending::settle_shorts(netuid(), &mut meter()));
        assert_eq!(
            read::<(u64, u64)>(b"test/returned"),
            (100_000, 100_000 - debt)
        );
        assert_eq!(
            Vaults::<Test>::get(netuid()).unwrap().outstanding_alpha,
            debt
        );
        assert!(!Lending::settle_remaining_longs(netuid(), &mut meter()));
        assert_ok!(Lending::freeze_redemption_basis(
            netuid(),
            3000.into(),
            1000,
            4
        ));
        assert_ok!(Lending::freeze_redemption_basis(
            netuid(),
            3000.into(),
            1000,
            4
        ));
        assert_noop!(
            Lending::freeze_redemption_basis(netuid(), 3001.into(), 1000, 4),
            Error::<Test>::InvalidQuote
        );
        // Ordinary free-alpha receipts belong to their holders, independently of
        // the loan. The principal still waits until all payout pages complete.
        assert_ok!(Lending::on_alpha_redemption(
            netuid(),
            &account(1),
            100.into()
        ));
        assert_eq!(position().principal, debt);
        let recovery_before = tao(&Lending::recovery_account());
        let basis = RedemptionBases::<Test>::get(netuid()).unwrap();
        assert!(Lending::settle_remaining_longs(netuid(), &mut meter()));
        let paid = 3 * debt + 4;
        let vault = Vaults::<Test>::get(netuid()).unwrap();
        assert_eq!(vault.available_tao, paid);
        assert_eq!(vault.available_alpha, 0);
        assert_eq!(vault.lost_alpha, 0);
        assert_eq!(tao(&account(1)), 999_000 + 1000 - paid);
        assert_eq!(alpha(&account(1), &account(1), netuid()), user_alpha);
        assert_eq!(RedemptionBases::<Test>::get(netuid()), Some(basis));
        assert_eq!(tao(&Lending::recovery_account()), recovery_before);
        assert_eq!(swap_count(), 0);
        assert_ok!(Lending::finish_dissolution(netuid()));
        assert_eq!(tao(&Lending::recovery_account()), recovery_before + paid);
        assert!(!RedemptionBases::<Test>::contains_key(netuid()));
        assert!(!Dissolutions::<Test>::contains_key(netuid()));
    });
}

#[test]
fn terminal_short_uses_higher_of_funded_redemption_and_frozen_ema() {
    for (fast, expected_paid) in [(1_u64, 751_u64), (4, 996)] {
        ext().execute_with(|| {
            open(Side::Short, 1000);
            assert_eq!(position().principal, 249);
            write(b"test/fast_price", Some(U64F64::from_num(fast)));
            assert_ok!(Lending::start_dissolution(netuid()));
            assert!(Lending::settle_shorts(netuid(), &mut meter()));
            assert_ok!(Lending::freeze_redemption_basis(
                netuid(),
                3000.into(),
                1000,
                4
            ));
            assert!(Lending::settle_remaining_longs(netuid(), &mut meter()));
            assert_eq!(
                Vaults::<Test>::get(netuid()).unwrap().available_tao,
                expected_paid
            );
            assert_eq!(Vaults::<Test>::get(netuid()).unwrap().lost_alpha, 0);
            assert_eq!(tao(&account(1)), 1_000_000 - expected_paid);
        });
    }
}

#[test]
fn donated_alpha_redemption_in_short_escrow_is_refunded_without_backing_debt() {
    ext().execute_with(|| {
        open(Side::Short, 1000);
        let debt = position().principal;
        let escrow = Lending::position_account(&account(1), netuid());
        let custody = Lending::custody_hotkey().unwrap();
        mint_alpha(&account(2), &account(2), netuid(), 100);
        assert_ok!(MockPool::transfer_staked_alpha(
            &account(2),
            &account(2),
            &escrow,
            &custody,
            netuid(),
            100.into(),
            false,
            false,
        ));
        assert_ok!(Lending::start_dissolution(netuid()));
        assert!(Lending::settle_shorts(netuid(), &mut meter()));
        assert_ok!(Lending::freeze_redemption_basis(
            netuid(),
            0.into(),
            1000,
            1
        ));
        // A public donation can make a short escrow an ordinary alpha holder.
        // Accept each funded receipt, without allowing it to cover principal.
        for receipt in [10_u64, 20] {
            mint_tao(&escrow, receipt);
            assert_ok!(Lending::on_alpha_redemption(
                netuid(),
                &escrow,
                receipt.into()
            ));
        }
        assert_eq!(position().proceeds, 30);
        assert_eq!(position().collateral, 1000);
        assert_eq!(position().principal, debt);
        assert!(Lending::settle_remaining_longs(netuid(), &mut meter()));
        assert_eq!(Vaults::<Test>::get(netuid()).unwrap().available_tao, debt);
        assert_eq!(tao(&account(1)), 1_000_000 - debt + 30);
        assert_eq!(tao(&escrow), 0);
        assert_eq!(Vaults::<Test>::get(netuid()).unwrap().lost_alpha, 0);
        assert!(!EscrowOwner::<Test>::contains_key(netuid(), &escrow));
    });
    ext().execute_with(|| {
        open(Side::Short, 1000);
        let debt = position().principal;
        let escrow = Lending::position_account(&account(1), netuid());
        write(b"test/fast_price", Some(U64F64::from_num(10)));
        assert_ok!(Lending::start_dissolution(netuid()));
        assert!(Lending::settle_shorts(netuid(), &mut meter()));
        assert_ok!(Lending::freeze_redemption_basis(
            netuid(),
            0.into(),
            1000,
            1
        ));
        mint_tao(&escrow, 5000);
        assert_ok!(Lending::on_alpha_redemption(netuid(), &escrow, 5000.into()));
        assert!(Lending::settle_remaining_longs(netuid(), &mut meter()));
        assert_eq!(Vaults::<Test>::get(netuid()).unwrap().available_tao, 1000);
        assert_eq!(
            Vaults::<Test>::get(netuid()).unwrap().lost_alpha,
            debt - 100
        );
        assert_eq!(tao(&account(1)), 999_000 + 5000);
    });
}

#[test]
fn terminal_funded_floor_shortfall_uses_only_available_collateral() {
    ext().execute_with(|| {
        open(Side::Short, 1000);
        let debt = position().principal;
        assert_eq!(debt, 249);
        assert_ok!(Lending::start_dissolution(netuid()));
        assert!(Lending::settle_shorts(netuid(), &mut meter()));
        assert_ok!(Lending::freeze_redemption_basis(
            netuid(),
            2000.into(),
            250,
            2
        ));
        assert!(Lending::settle_remaining_longs(netuid(), &mut meter()));
        let vault = Vaults::<Test>::get(netuid()).unwrap();
        assert_eq!(vault.available_tao, 1000);
        assert_eq!(vault.lost_alpha, debt - debt * 1000 / 1994);
        assert_eq!(vault.outstanding_alpha, 0);
        assert_eq!(vault.available_alpha, 0);
        assert_eq!(tao(&account(1)), 999_000);
        assert_eq!(tao(&Lending::position_account(&account(1), netuid())), 0);
        assert_eq!(swap_count(), 0);
    });
}

#[test]
fn terminal_funded_rounding_handles_zero_basis_split_rows_and_overlarge_debt() {
    ext().execute_with(|| {
        for (amount, basis, expected) in [
            (5_u64, (100_u64, 10_u128, 3_u64), 53_u128),
            (5, (101, 10, 3), 54),
            (3, (1, 100, 100), 1),
            (20, (10, 10, 100), 20),
            (5, (100, 0, 100), 0),
            (5, (0, 10, 100), 0),
            (
                u64::MAX,
                (u64::MAX, 1, u64::MAX),
                u128::from(u64::MAX).pow(2),
            ),
        ] {
            assert_eq!(
                Lending::funded_alpha_value(amount, basis).unwrap(),
                expected
            );
        }
        assert_eq!(
            Lending::marked_alpha_value(3, U64F64::from_num(0.5)).unwrap(),
            2
        );
        assert_eq!(
            Lending::marked_alpha_value(u64::MAX, U64F64::from_bits(u128::MAX)).unwrap(),
            u128::MAX - u128::from(u64::MAX)
        );
    });
}

#[test]
fn terminal_basis_is_shared_unchanged_across_multiple_alpha_loan_recoveries() {
    ext().execute_with(|| {
        open(Side::Short, 1000);
        let debt = position().principal;
        mint_tao(&account(2), 1000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(account(2)),
            netuid(),
            Side::Short,
            1000,
            account(2),
            1,
            0
        ));
        assert_ok!(Lending::start_dissolution(netuid()));
        assert!(Lending::settle_shorts(netuid(), &mut meter()));
        assert_ok!(Lending::freeze_redemption_basis(
            netuid(),
            1000.into(),
            1000,
            2
        ));
        let mut budget = WeightMeter::with_limit(Weight::from_parts(1, 0));
        assert!(!Lending::settle_remaining_longs(netuid(), &mut budget));
        assert_eq!(TotalPositions::<Test>::get(), 1);
        assert_eq!(
            Vaults::<Test>::get(netuid()).unwrap().available_tao,
            debt + 2
        );
        assert_eq!(
            RedemptionBases::<Test>::get(netuid()),
            Some((1000, 1000, 2))
        );
        assert!(Lending::settle_remaining_longs(netuid(), &mut meter()));
        assert_eq!(
            Vaults::<Test>::get(netuid()).unwrap().available_tao,
            2 * (debt + 2)
        );
        assert_eq!(
            RedemptionBases::<Test>::get(netuid()),
            Some((1000, 1000, 2))
        );
        assert_eq!(TotalPositions::<Test>::get(), 0);
        assert_eq!(PositionCount::<Test>::get(netuid()), 0);
        assert_eq!(Due::<Test>::iter().count(), 0);
    });
}

#[test]
fn zero_ordinary_alpha_supply_still_settles_the_frozen_ema_debt() {
    ext().execute_with(|| {
        open(Side::Short, 1000);
        let debt = position().principal;
        assert_ok!(Lending::start_dissolution(netuid()));
        assert!(Lending::settle_shorts(netuid(), &mut meter()));
        assert_ok!(Lending::freeze_redemption_basis(
            netuid(),
            u64::MAX.into(),
            0,
            0
        ));
        assert!(Lending::settle_remaining_longs(netuid(), &mut meter()));
        assert_eq!(Vaults::<Test>::get(netuid()).unwrap().available_tao, debt);
        assert_eq!(Vaults::<Test>::get(netuid()).unwrap().lost_alpha, 0);
        assert_eq!(tao(&account(1)), 1_000_000 - debt);
        assert_ok!(Lending::finish_dissolution(netuid()));
        assert!(!RedemptionBases::<Test>::contains_key(netuid()));
    });
}

#[test]
fn metered_terminal_freeze_and_settlement_cover_both_loan_sides() {
    ext().execute_with(|| {
        open(Side::Short, 1000);
        mint_alpha(&account(2), &account(2), netuid(), 1000);
        assert_ok!(Lending::open(
            RuntimeOrigin::signed(account(2)),
            netuid(),
            Side::Long,
            1000,
            account(2),
            1,
            0
        ));
        System::set_block_number(6);
        assert_ok!(Lending::start_dissolution(netuid()));
        System::set_block_number(1000);
        let mut frozen = false;
        for _ in 0..3 {
            frozen = Lending::settle_shorts(
                netuid(),
                &mut WeightMeter::with_limit(Weight::from_parts(1, 0)),
            );
            if frozen {
                break;
            }
        }
        assert!(frozen);
        for n in [1, 2] {
            let p = Positions::<Test>::get(account(n), netuid()).unwrap();
            assert_eq!(p.last_accrued, 6);
            assert_eq!(p.interest_remainder, 0);
            assert_eq!(p.collateral, 997);
        }
        let short = position();
        let long = Positions::<Test>::get(account(2), netuid()).unwrap();
        let escrow = Lending::position_account(&account(2), netuid());
        mint_tao(&escrow, 50);
        assert_ok!(Lending::freeze_redemption_basis(
            netuid(),
            0.into(),
            1000,
            2
        ));
        assert_ok!(Lending::on_alpha_redemption(netuid(), &escrow, 50.into()));
        let mut settled = false;
        for _ in 0..3 {
            settled = Lending::settle_remaining_longs(
                netuid(),
                &mut WeightMeter::with_limit(Weight::from_parts(1, 0)),
            );
            if settled {
                break;
            }
        }
        assert!(settled);
        let vault = Vaults::<Test>::get(netuid()).unwrap();
        assert_eq!(vault.available_tao, short.principal + 50);
        assert_eq!(vault.lost_tao, long.principal - 50);
        assert_eq!(vault.outstanding_tao, 0);
        assert_eq!(vault.outstanding_alpha, 0);
        assert_eq!(vault.pending_alpha, 0);
        assert_eq!(TotalPositions::<Test>::get(), 0);
        assert_eq!(Due::<Test>::iter().count(), 0);
        assert_ok!(Lending::finish_dissolution(netuid()));
    });
}

#[test]
fn long_deregistration_recovers_only_actual_funded_redemption() {
    for payout in [20_u64, 700] {
        ext().execute_with(|| {
            open(Side::Long, 1000);
            let first = position().principal;
            open(Side::Long, 1000);
            let debt = position().principal;
            assert!(debt > first);
            assert_eq!(PositionCount::<Test>::get(netuid()), 1);
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
        // The alpha principal must also fit the actual full reference purchase.
        write(b"test/fail_buy", true);
        assert!(
            Lending::open(
                RuntimeOrigin::signed(account(1)),
                netuid(),
                Side::Short,
                1000,
                account(1),
                short.principal,
                0,
            )
            .is_err()
        );
        assert!(Positions::<Test>::get(account(1), netuid()).is_none());
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
        open(Side::Long, 100);
        assert_eq!(TotalPositions::<Test>::get(), 5);
        assert_eq!(PositionCount::<Test>::get(netuid()), 4);
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
        write(b"test/fast_price", Some(U64F64::from_bits(u128::MAX)));
        assert_ok!(Lending::start_dissolution(netuid()));
        assert!(Lending::settle_shorts(netuid(), &mut meter()));
        assert_ok!(Lending::freeze_redemption_basis(netuid(), 0.into(), 0, 0));
        assert!(Lending::settle_remaining_longs(netuid(), &mut meter()));
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
        assert_ok!(Lending::freeze_redemption_basis(netuid(), 0.into(), 0, 0));
        assert!(Lending::settle_remaining_longs(netuid(), &mut meter()));
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
