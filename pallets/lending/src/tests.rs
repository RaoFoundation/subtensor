#![allow(
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing
)]

//! Focused tests for synthetic-mint shorts (SPEC v1 §6).

use super::*;
use crate::weights::WeightInfo;
use codec::{Decode, Encode};
use frame_support::{
    assert_err, assert_noop, assert_ok, construct_runtime, derive_impl, ensure,
    traits::{ConstU32, ConstU64, Hooks},
    weights::{Weight, WeightMeter},
};
use sp_runtime::{
    AccountId32, BuildStorage, DispatchError,
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

frame_support::parameter_types! {
    pub const LendingId: PalletId = PalletId(*b"bt/lends");
}

/// 100 blocks per day.
const BLOCKS_PER_YEAR: u64 = 36_500;

impl Config for Test {
    type Pool = MockPool;
    type PalletId = LendingId;
    type InterestPeriod = ConstU64<10>;
    type BlocksPerYear = ConstU64<BLOCKS_PER_YEAR>;
    type MaxPositionsPerSubnet = ConstU32<8>;
    type MaxTotalPositions = ConstU32<16>;
    type InterestRetryDelay = ConstU64<5>;
    type WeightInfo = ();
}

const POOL: u64 = 1_000_000_000_000_000;
const ISSUANCE: u64 = 10_000_000_000_000_000;
const MINT: u64 = 1_000_000_000_000;
const COLLATERAL: u64 = 2_500_000_000_000;
const HALF_WEIGHT: u64 = 500_000_000_000_000_000;

fn account(n: u8) -> Account {
    Account::new([n; 32])
}
fn burn_account() -> Account {
    account(255)
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
fn set_tao(who: &Account, amount: u64) {
    write(&key(b"test/tao", who), amount);
}
fn mint_tao(who: &Account, amount: u64) {
    set_tao(who, tao(who).saturating_add(amount));
}
fn pool() -> (u64, u64) {
    (
        read(&key(b"test/pool_tao", netuid())),
        read(&key(b"test/pool_alpha", netuid())),
    )
}
fn set_pool(tao_r: u64, alpha_r: u64) {
    write(&key(b"test/pool_tao", netuid()), tao_r);
    write(&key(b"test/pool_alpha", netuid()), alpha_r);
}
fn issuance() -> u64 {
    read(&key(b"test/issuance", netuid()))
}
fn set_issuance(v: u64) {
    write(&key(b"test/issuance", netuid()), v);
    write(&key(b"test/base_issuance", netuid()), v);
}
fn base_issuance() -> u64 {
    read(&key(b"test/base_issuance", netuid()))
}
fn burned_alpha() -> u64 {
    read(&key(b"test/burned_alpha", netuid()))
}
fn pot() -> u64 {
    read(&key(b"test/pot", netuid()))
}
fn set_ema(price: f64) {
    write(&key(b"test/ema", netuid()), U64F64::from_num(price));
}
fn set_live(live: bool) {
    write(&key(b"test/dead", netuid()), !live);
}
fn spot() -> f64 {
    MockPool::current_alpha_price(netuid()).to_num::<f64>()
}

pub struct MockPool;

impl MockPool {
    fn buy_out(y: u64, x: u64, t: u64) -> u64 {
        (u128::from(x) * u128::from(t) / u128::from(y.saturating_add(t))) as u64
    }
    fn sell_out(y: u64, x: u64, a: u64) -> u64 {
        (u128::from(y) * u128::from(a) / u128::from(x.saturating_add(a))) as u64
    }
}

impl OrderSwapInterface<Account> for MockPool {
    fn buy_alpha(
        _: &Account,
        _: &Account,
        _: NetUid,
        _: TaoBalance,
        _: TaoBalance,
        _: bool,
    ) -> Result<AlphaBalance, DispatchError> {
        Err("unused".into())
    }

    fn sell_alpha(
        _: &Account,
        _: &Account,
        _: NetUid,
        _: AlphaBalance,
        _: TaoBalance,
        _: bool,
    ) -> Result<TaoBalance, DispatchError> {
        Err("unused".into())
    }

    fn current_alpha_price(_: NetUid) -> U64F64 {
        let (y, x) = pool();
        if x == 0 {
            return U64F64::from_num(0);
        }
        U64F64::saturating_from_num(y) / U64F64::saturating_from_num(x)
    }

    fn transfer_tao(from: &Account, to: &Account, amount: TaoBalance) -> DispatchResult {
        let a = amount.to_u64();
        ensure!(tao(from) >= a, "insufficient tao");
        set_tao(from, tao(from) - a);
        mint_tao(to, a);
        Ok(())
    }

    fn transfer_staked_alpha(
        _: &Account,
        _: &Account,
        _: &Account,
        _: &Account,
        _: NetUid,
        _: AlphaBalance,
        _: bool,
        _: bool,
    ) -> DispatchResult {
        Err("unused".into())
    }

    fn register_pallet_hotkey(_: &Account, _: &Account) -> DispatchResult {
        Ok(())
    }

    fn pallet_hotkey_registered(_: &Account, _: &Account) -> bool {
        true
    }
}

/// Fee-free 50/50 constant-product pool with an alpha issuance counter.
impl LendingPoolInterface<Account> for MockPool {
    fn subnet_exists(netuid: NetUid) -> bool {
        !read::<bool>(&key(b"test/dead", netuid))
    }
    fn quote_buy(_: NetUid, t: TaoBalance) -> Result<AlphaBalance, DispatchError> {
        let (y, x) = pool();
        Ok(Self::buy_out(y, x, t.to_u64()).into())
    }
    fn quote_sell(_: NetUid, a: AlphaBalance) -> Result<TaoBalance, DispatchError> {
        let (y, x) = pool();
        Ok(Self::sell_out(y, x, a.to_u64()).into())
    }
    fn pool_tao(_: NetUid) -> TaoBalance {
        pool().0.into()
    }
    fn pool_alpha(_: NetUid) -> AlphaBalance {
        pool().1.into()
    }
    fn quote_weight_parts(_: NetUid) -> u64 {
        HALF_WEIGHT
    }
    fn quote_sell_on(y: TaoBalance, x: AlphaBalance, w: u64, a: AlphaBalance) -> TaoBalance {
        assert_eq!(w, HALF_WEIGHT);
        Self::sell_out(y.to_u64(), x.to_u64(), a.to_u64()).into()
    }
    fn quote_buy_on(y: TaoBalance, x: AlphaBalance, w: u64, t: TaoBalance) -> AlphaBalance {
        assert_eq!(w, HALF_WEIGHT);
        Self::buy_out(y.to_u64(), x.to_u64(), t.to_u64()).into()
    }
    fn alpha_issuance(_: NetUid) -> AlphaBalance {
        issuance().into()
    }
    fn moving_alpha_price(netuid: NetUid) -> U64F64 {
        read(&key(b"test/ema", netuid))
    }
    fn live_subnets() -> Vec<NetUid> {
        vec![netuid()]
    }
    fn mint_and_sell(
        netuid: NetUid,
        custody_cold: &Account,
        _: &Account,
        mint: AlphaBalance,
        min_tao_out: TaoBalance,
    ) -> Result<TaoBalance, DispatchError> {
        let m = mint.to_u64();
        let (y, x) = pool();
        let t = Self::sell_out(y, x, m);
        ensure!(t > 0, "zero out");
        ensure!(t >= min_tao_out.to_u64(), "slippage");
        write(&key(b"test/issuance", netuid), issuance() + m);
        set_pool(y - t, x + m);
        mint_tao(custody_cold, t);
        Ok(t.into())
    }
    fn buy_and_burn(
        netuid: NetUid,
        custody_cold: &Account,
        _: &Account,
        alpha: AlphaBalance,
        max_tao_in: TaoBalance,
    ) -> Result<TaoBalance, DispatchError> {
        let a = alpha.to_u64();
        let (y, x) = pool();
        ensure!(a < x, "insufficient pool alpha");
        let t = (u128::from(a) * u128::from(y)).div_ceil(u128::from(x - a)) as u64;
        ensure!(t <= max_tao_in.to_u64(), "slippage");
        ensure!(tao(custody_cold) >= t, "insufficient tao");
        let bought = Self::buy_out(y, x, t);
        ensure!(bought >= a, "short fill");
        set_tao(custody_cold, tao(custody_cold) - t);
        set_pool(y + t, x - bought);
        write(&key(b"test/issuance", netuid), issuance() - bought);
        write(&key(b"test/burned_alpha", netuid), burned_alpha() + bought);
        Ok(t.into())
    }
    fn burn_interest_tao(account: &Account, amount: TaoBalance) -> DispatchResult {
        Self::transfer_tao(account, &burn_account(), amount)
    }
    fn pay_into_pot(
        from: &Account,
        netuid: NetUid,
        amount: TaoBalance,
    ) -> Result<TaoBalance, DispatchError> {
        let a = amount.to_u64();
        ensure!(tao(from) >= a, "insufficient tao");
        set_tao(from, tao(from) - a);
        write(&key(b"test/pot", netuid), pot() + a);
        Ok(amount)
    }
}

fn new_test_ext() -> sp_io::TestExternalities {
    let mut ext: sp_io::TestExternalities = frame_system::GenesisConfig::<Test>::default()
        .build_storage()
        .unwrap()
        .into();
    ext.execute_with(|| {
        System::set_block_number(10);
        Enabled::<Test>::put(true);
        set_pool(POOL, POOL);
        set_issuance(ISSUANCE);
        set_ema(1.0);
    });
    ext
}

/// Run `f`, then assert the try-state invariants and the mock-side balances.
fn run(f: impl FnOnce()) {
    new_test_ext().execute_with(|| {
        f();
        check_invariants();
    });
}

/// I1 via `do_try_state`, I2 issuance reconciliation, I3 custody and reserve
/// balances, I4 coverage for open positions, I5 collateral never negative.
fn check_invariants() {
    assert_ok!(Lending::do_try_state());
    if Dissolutions::<Test>::get(netuid()).is_none() && !read::<bool>(b"test/dissolved") {
        let synthetic = SyntheticOutstanding::<Test>::get(netuid());
        let residual = ResidualAlpha::<Test>::get(netuid());
        assert_eq!(
            issuance(),
            base_issuance() + synthetic + residual,
            "I2: issuance must equal base + synthetic + residual"
        );
    }
    for (_, n, pos) in Positions::<Test>::iter() {
        assert_eq!(tao(&pos.custody), pos.funds(), "I3: custody TAO");
        let _ = n;
    }
    for (n, r) in ResidualReserve::<Test>::iter() {
        assert_eq!(tao(&Lending::reserve_account(n)), r, "I3: reserve TAO");
    }
}

fn params() -> SynthParams {
    SynthParamsStore::<Test>::get()
}

fn open(who: u8, mint: u64, collateral: u64) -> DispatchResult {
    let owner = account(who);
    mint_tao(&owner, collateral);
    Lending::open_short(RuntimeOrigin::signed(owner), netuid(), mint, collateral, 0)
}

fn position(who: u8) -> ShortPosition<Account, u64> {
    Positions::<Test>::get(account(who), netuid()).expect("position")
}

fn run_to(block: u64) {
    while System::block_number() < block {
        let next = System::block_number() + 1;
        System::set_block_number(next);
        Lending::on_initialize(next);
        Lending::on_idle(next, Weight::MAX);
    }
}

fn events() -> Vec<Event<Test>> {
    System::events()
        .into_iter()
        .filter_map(|r| match r.event {
            RuntimeEvent::Lending(e) => Some(e),
            _ => None,
        })
        .collect()
}

// ---- open / close ----------------------------------------------------------

#[test]
fn open_close_round_trip_costs_only_rounding() {
    run(|| {
        let (y0, x0) = pool();
        assert_ok!(open(1, MINT, COLLATERAL));
        let pos = position(1);
        assert_eq!(pos.minted, MINT);
        assert_eq!(pos.collateral, COLLATERAL);
        assert!(pos.proceeds > 0);
        assert_eq!(SyntheticOutstanding::<Test>::get(netuid()), MINT);
        assert_eq!(issuance(), ISSUANCE + MINT);
        assert_eq!(tao(&account(1)), 0);
        check_invariants();

        assert_ok!(Lending::close_short(
            RuntimeOrigin::signed(account(1)),
            netuid(),
            MINT,
            u64::MAX
        ));
        assert!(Positions::<Test>::get(account(1), netuid()).is_none());
        assert_eq!(SyntheticOutstanding::<Test>::get(netuid()), 0);
        assert_eq!(issuance(), ISSUANCE);
        assert_eq!(burned_alpha(), MINT);
        let refund = tao(&account(1));
        assert!(
            COLLATERAL - refund <= 2,
            "round trip lost {}",
            COLLATERAL - refund
        );
        let (y1, x1) = pool();
        assert_eq!(x1, x0);
        assert!(y1 >= y0 && y1 - y0 <= 2);
        assert!(matches!(
            events().last(),
            Some(Event::ShortClosed { refund: r, .. }) if *r == refund
        ));
    });
}

#[test]
fn partial_close_and_increase_keep_coverage() {
    run(|| {
        assert_ok!(open(1, MINT, COLLATERAL));
        System::set_block_number(11);
        assert_ok!(Lending::close_short(
            RuntimeOrigin::signed(account(1)),
            netuid(),
            MINT / 2,
            u64::MAX
        ));
        assert_eq!(position(1).minted, MINT / 2);
        check_invariants();

        mint_tao(&account(1), COLLATERAL);
        assert_ok!(Lending::increase_short(
            RuntimeOrigin::signed(account(1)),
            netuid(),
            MINT / 2,
            COLLATERAL,
            0
        ));
        let pos = position(1);
        assert_eq!(pos.minted, MINT);
        assert_eq!(SyntheticOutstanding::<Test>::get(netuid()), MINT);
        assert!(Lending::coverage_cost(netuid(), pos.minted, &params()).unwrap() <= pos.funds());
    });
}

#[test]
fn open_rejects_existing_position_and_zero_mint() {
    run(|| {
        assert_ok!(open(1, MINT, COLLATERAL));
        assert_err!(open(1, MINT, COLLATERAL), Error::<Test>::PositionExists);
        assert_err!(open(2, 0, COLLATERAL), Error::<Test>::ZeroAmount);
    });
}

// ---- coverage --------------------------------------------------------------

#[test]
fn quote_required_collateral_is_the_coverage_boundary() {
    run(|| {
        let quote = Lending::quote_open_short(netuid(), MINT).unwrap();
        assert_eq!(quote.notional, MINT);
        assert_err!(
            open(1, MINT, quote.required_collateral - 1),
            Error::<Test>::CoverageNotMet
        );
        assert_ok!(open(1, MINT, quote.required_collateral));
        let pos = position(1);
        assert_eq!(pos.proceeds, quote.proceeds);
        let cost = Lending::coverage_cost(netuid(), MINT, &params()).unwrap();
        assert_eq!(pos.funds(), cost);
        // About 3× notional must be in custody at the default 3× stress.
        assert!(cost > 3 * MINT && cost < 31 * MINT / 10, "{cost}");
    });
}

#[test]
fn coverage_queues_behind_other_shorts() {
    run(|| {
        let alone = Lending::quote_open_short(netuid(), MINT).unwrap();
        assert_ok!(open(1, MINT * 5, 20 * MINT));
        let behind = Lending::quote_open_short(netuid(), MINT).unwrap();
        assert!(
            behind.required_collateral > alone.required_collateral,
            "queued coverage must cost more"
        );
    });
}

#[test]
fn coverage_uses_max_of_ema_and_spot() {
    run(|| {
        let at_spot = Lending::quote_open_short(netuid(), MINT).unwrap();
        set_ema(2.0);
        let at_ema = Lending::quote_open_short(netuid(), MINT).unwrap();
        assert!(at_ema.required_collateral > at_spot.required_collateral * 2);
    });
}

#[test]
fn withdraw_blocked_when_coverage_would_break() {
    run(|| {
        assert_ok!(open(1, MINT, COLLATERAL));
        let pos = position(1);
        let cost = Lending::coverage_cost(netuid(), MINT, &params()).unwrap();
        let slack = pos.funds() - cost;
        System::set_block_number(11);
        assert_noop!(
            Lending::withdraw_collateral(RuntimeOrigin::signed(account(1)), netuid(), slack + 1),
            Error::<Test>::WithdrawBreaksCoverage
        );
        assert_ok!(Lending::withdraw_collateral(
            RuntimeOrigin::signed(account(1)),
            netuid(),
            slack / 2
        ));
        assert_eq!(tao(&account(1)), slack / 2);
    });
}

// ---- supply cap, minimum, impact -----------------------------------------------

#[test]
fn supply_cap_exact_boundary() {
    run(|| {
        set_issuance(5 * MINT);
        let cap = MINT; // 20% of 5 × MINT
        assert_err!(open(1, cap + 1, 10 * cap), Error::<Test>::SupplyCapExceeded);
        assert_ok!(open(1, cap, 10 * cap));
        mint_tao(&account(1), 10 * cap);
        assert_noop!(
            Lending::increase_short(RuntimeOrigin::signed(account(1)), netuid(), 1, 0, 0),
            Error::<Test>::SupplyCapExceeded
        );
        // Issuance_ex excludes the synthetic alpha just minted.
        assert_eq!(Lending::issuance_ex(netuid()), u128::from(5 * MINT));
    });
}

#[test]
fn minimum_notional_boundary() {
    run(|| {
        let min = params().min_notional;
        assert_err!(
            open(1, min - 1, COLLATERAL),
            Error::<Test>::BelowMinNotional
        );
        assert_ok!(open(1, min, COLLATERAL));
        // A partial close may not leave dust below the minimum.
        assert_noop!(
            Lending::close_short(RuntimeOrigin::signed(account(1)), netuid(), 1, u64::MAX),
            Error::<Test>::BelowMinNotional
        );
        set_ema(0.5);
        assert_err!(open(2, min, COLLATERAL), Error::<Test>::BelowMinNotional);
    });
}

#[test]
fn open_impact_budget_is_per_block() {
    run(|| {
        // Each 7.5e12 sale drops spot by about 1.5% of a 1e15 pool.
        let big = 7_500_000_000_000;
        assert_ok!(open(1, big, 40 * big));
        assert!(spot() < 0.986 && spot() > 0.98);
        assert_err!(open(2, big, 40 * big), Error::<Test>::OpenImpactTooHigh);
        System::set_block_number(11);
        assert_ok!(open(2, big, 40 * big));
    });
}

#[test]
fn slippage_guard() {
    run(|| {
        let quote = Lending::quote_open_short(netuid(), MINT).unwrap();
        mint_tao(&account(1), COLLATERAL);
        assert_noop!(
            Lending::open_short(
                RuntimeOrigin::signed(account(1)),
                netuid(),
                MINT,
                COLLATERAL,
                quote.proceeds + 1
            ),
            Error::<Test>::SlippageExceeded
        );
    });
}

// ---- interest ----------------------------------------------------------------

fn expected_interest(minted: u64, blocks: u64, remainder: u64) -> (u64, u64) {
    let notional = u128::from(minted);
    let apr = u128::from(Lending::apr_bps(netuid(), &params()));
    let num = notional * apr * u128::from(blocks) + u128::from(remainder);
    let den = 10_000u128 * u128::from(BLOCKS_PER_YEAR);
    ((num / den) as u64, (num % den) as u64)
}

#[test]
fn interest_splits_between_burn_and_reserve() {
    run(|| {
        assert_ok!(open(1, MINT, COLLATERAL));
        let (owed, rem) = expected_interest(MINT, 10, 0);
        run_to(20);
        let pos = position(1);
        assert_eq!(pos.collateral, COLLATERAL - owed);
        assert_eq!(pos.interest_remainder, rem);
        assert_eq!(pos.due, 30);
        let reserved = owed * 5_000 / 10_000;
        assert_eq!(ResidualReserve::<Test>::get(netuid()), reserved);
        assert_eq!(tao(&burn_account()), owed - reserved);
        assert!(events().contains(&Event::InterestCharged {
            who: account(1),
            netuid: netuid(),
            burned: owed - reserved,
            reserved,
        }));
    });
}

#[test]
fn reserve_cap_overflow_burns() {
    run(|| {
        assert_ok!(open(1, MINT, COLLATERAL));
        let cap = pool().0 * 200 / 10_000;
        let reserve = Lending::reserve_account(netuid());
        mint_tao(&reserve, cap - 1_000);
        ResidualReserve::<Test>::insert(netuid(), cap - 1_000);
        let (owed, _) = expected_interest(MINT, 10, 0);
        run_to(20);
        assert_eq!(ResidualReserve::<Test>::get(netuid()), cap);
        assert_eq!(tao(&burn_account()), owed - 1_000);
    });
}

#[test]
fn interest_remainder_carries_across_charges() {
    run(|| {
        let min = params().min_notional;
        assert_ok!(open(1, min, COLLATERAL));
        let (o1, r1) = expected_interest(min, 10, 0);
        run_to(20);
        assert_eq!(position(1).interest_remainder, r1);
        let (o2, r2) = expected_interest(min, 10, r1);
        run_to(30);
        let pos = position(1);
        assert_eq!(pos.collateral, COLLATERAL - o1 - o2);
        assert_eq!(pos.interest_remainder, r2);
    });
}

#[test]
fn apr_rises_to_ceiling_at_knee() {
    run(|| {
        let p = params();
        assert_eq!(Lending::apr_bps(netuid(), &p), p.apr_floor_bps);
        // Knee = 20% of Issuance_ex; half the knee gives the midpoint APR.
        let half_knee = ISSUANCE / 10;
        SyntheticOutstanding::<Test>::insert(netuid(), half_knee);
        set_issuance(ISSUANCE + half_knee);
        write(&key(b"test/base_issuance", netuid()), ISSUANCE);
        assert_eq!(Lending::apr_bps(netuid(), &p), 6_000);
        SyntheticOutstanding::<Test>::insert(netuid(), 2 * half_knee);
        set_issuance(ISSUANCE + 2 * half_knee);
        assert_eq!(Lending::apr_bps(netuid(), &p), p.apr_ceil_bps);
        SyntheticOutstanding::<Test>::insert(netuid(), 3 * half_knee);
        set_issuance(ISSUANCE + 3 * half_knee);
        assert_eq!(Lending::apr_bps(netuid(), &p), p.apr_ceil_bps);
        SyntheticOutstanding::<Test>::remove(netuid());
        set_issuance(ISSUANCE);
    });
}

#[test]
fn exhausted_collateral_starts_deleverage() {
    run(|| {
        assert_ok!(open(1, MINT, COLLATERAL));
        SynthParamsStore::<Test>::mutate(|p| {
            p.apr_floor_bps = MAX_APR_BPS;
            p.apr_ceil_bps = MAX_APR_BPS;
        });
        // 1000% APR on 1e12 for 200 days far exceeds the collateral.
        System::set_block_number(20_000);
        Lending::on_idle(20_000, Weight::MAX);
        let pos = position(1);
        assert_eq!(pos.collateral, 0, "I5: charge capped at collateral");
        assert!(matches!(pos.state, ShortState::Deleveraging { .. }));
        assert!(DeleverageQueue::<Test>::contains_key((
            account(1),
            netuid()
        )));
    });
}

#[test]
fn failed_charge_retries_later() {
    run(|| {
        assert_ok!(open(1, MINT, COLLATERAL));
        let custody = position(1).custody;
        // Drain custody behind the pallet's back so the charge fails.
        let held = tao(&custody);
        set_tao(&custody, 0);
        run_to(20);
        let pos = position(1);
        assert_eq!(pos.collateral, COLLATERAL);
        assert_eq!(pos.due, 25);
        set_tao(&custody, held);
        run_to(25);
        assert!(position(1).collateral < COLLATERAL);
    });
}

// ---- TWAP -------------------------------------------------------------------

#[test]
fn twap_ring_averages_and_wraps() {
    run(|| {
        SynthParamsStore::<Test>::mutate(|p| {
            p.twap_samples = 3;
            p.twap_interval_blocks = 10;
        });
        assert_eq!(Lending::twap(netuid()).to_num::<f64>(), 1.0);
        for (block, price) in [(20u64, 1.0f64), (30, 2.0), (40, 3.0)] {
            set_pool((POOL as f64 * price) as u64, POOL);
            run_to(block);
        }
        assert_eq!(TwapRing::<Test>::get(netuid()).samples.len(), 3);
        assert!((Lending::twap(netuid()).to_num::<f64>() - 2.0).abs() < 1e-9);
        // A one-block spike moves the mean by a third of its size.
        set_pool(POOL * 10, POOL);
        run_to(50);
        assert!((Lending::twap(netuid()).to_num::<f64>() - 5.0).abs() < 1e-9);
        assert_eq!(TwapRing::<Test>::get(netuid()).cursor, 1);
        // A re-registered netuid starts clean.
        <Lending as LendingInterface<Account>>::on_subnet_registered(netuid());
        assert!(TwapRing::<Test>::get(netuid()).samples.is_empty());
        set_pool(POOL, POOL);
    });
}

#[test]
fn twap_smooths_a_spike_out_of_equity() {
    run(|| {
        SynthParamsStore::<Test>::mutate(|p| {
            p.twap_samples = 72;
            p.twap_interval_blocks = 10;
        });
        assert_ok!(open(1, MINT, COLLATERAL));
        for b in 1..=72u64 {
            run_to(10 + b * 10);
        }
        // Spot spikes 4×; the TWAP still sits near 1, so equity stays positive.
        set_pool(POOL * 2, POOL / 2);
        let pos = position(1);
        assert!(Lending::equity_twap(netuid(), &pos) > 0);
        assert_ok!(Lending::poke(
            RuntimeOrigin::signed(account(9)),
            account(1),
            netuid()
        ));
        assert_eq!(position(1).state, ShortState::Open);
        set_pool(POOL, POOL);
    });
}

// ---- deleverage -------------------------------------------------------------------

fn pump_and_deleverage() {
    assert_ok!(open(1, MINT, COLLATERAL));
    // Spot 5×: buying 1e12 back costs about 5e12 > funds.
    let (y, x) = pool();
    set_pool(y * 5, x);
    assert_ok!(Lending::poke(
        RuntimeOrigin::signed(account(9)),
        account(1),
        netuid()
    ));
    assert!(matches!(position(1).state, ShortState::Deleveraging { .. }));
    assert!(matches!(
        events().last(),
        Some(Event::DeleverageStarted { equity_twap, .. }) if *equity_twap < 0
    ));
}

#[test]
fn deleverage_respects_pace_and_defaults_with_residual() {
    run(|| {
        pump_and_deleverage();
        let pace = f64::from(params().pace_bps_per_block) / 10_000.0;
        let mut blocks = 0;
        while Positions::<Test>::contains_key(account(1), netuid()) {
            let before = spot();
            let next = System::block_number() + 1;
            System::set_block_number(next);
            Lending::on_initialize(next);
            assert!(
                spot() <= before * (1.0 + pace) * (1.0 + 1e-12),
                "I9: pace exceeded"
            );
            check_invariants();
            blocks += 1;
            assert!(blocks < 1_000, "deleverage must finish");
        }
        assert!(blocks > 1, "deleverage must be partial across blocks");
        let residual = ResidualAlpha::<Test>::get(netuid());
        assert!(residual > 0);
        assert_eq!(SyntheticOutstanding::<Test>::get(netuid()), 0);
        assert_eq!(burned_alpha() + residual, MINT);
        assert!(events().iter().any(|e| matches!(
            e,
            Event::ShortDefaulted { residual_alpha, .. } if *residual_alpha == residual
        )));
    });
}

#[test]
fn deleveraging_position_cannot_grow_or_withdraw_but_can_close() {
    run(|| {
        pump_and_deleverage();
        mint_tao(&account(1), COLLATERAL);
        assert_noop!(
            Lending::increase_short(RuntimeOrigin::signed(account(1)), netuid(), 0, 1, 0),
            Error::<Test>::PositionDeleveraging
        );
        assert_noop!(
            Lending::withdraw_collateral(RuntimeOrigin::signed(account(1)), netuid(), 1),
            Error::<Test>::PositionDeleveraging
        );
        assert_ok!(Lending::add_collateral(
            RuntimeOrigin::signed(account(1)),
            netuid(),
            COLLATERAL
        ));
        assert_ok!(Lending::close_short(
            RuntimeOrigin::signed(account(1)),
            netuid(),
            MINT,
            u64::MAX
        ));
        assert!(!DeleverageQueue::<Test>::contains_key((
            account(1),
            netuid()
        )));
    });
}

#[test]
fn reserve_retires_residual_under_pace() {
    run(|| {
        pump_and_deleverage();
        run_to(System::block_number() + 200);
        assert!(!Positions::<Test>::contains_key(account(1), netuid()));
        let residual = ResidualAlpha::<Test>::get(netuid());
        assert!(residual > 0);
        let reserve = Lending::reserve_account(netuid());
        let funding = 100 * MINT;
        mint_tao(&reserve, funding);
        ResidualReserve::<Test>::mutate(netuid(), |r| *r += funding);
        let pace = f64::from(params().pace_bps_per_block) / 10_000.0;
        let before = spot();
        let next = System::block_number() + 1;
        System::set_block_number(next);
        Lending::on_initialize(next);
        assert!(spot() <= before * (1.0 + pace) * (1.0 + 1e-12));
        let after = ResidualAlpha::<Test>::get(netuid());
        assert!(after < residual);
        check_invariants();
        run_to(next + 200);
        assert_eq!(ResidualAlpha::<Test>::get(netuid()), 0);
    });
}

// ---- dissolution -------------------------------------------------------------------

#[test]
fn dissolution_settles_at_ema_once_with_resuming_cursor() {
    run(|| {
        assert_ok!(open(1, MINT, COLLATERAL));
        assert_ok!(open(2, MINT, 2 * COLLATERAL));
        let reserve = Lending::reserve_account(netuid());
        mint_tao(&reserve, 7);
        ResidualReserve::<Test>::insert(netuid(), 7);
        ResidualAlpha::<Test>::insert(netuid(), 3);
        write(&key(b"test/issuance", netuid()), issuance() + 3);
        let funds: Vec<u64> = [1u8, 2].iter().map(|w| position(*w).funds()).collect();
        set_ema(1.5);
        assert_ok!(Lending::start_dissolution(netuid()));
        assert_eq!(
            Dissolutions::<Test>::get(netuid()).unwrap().price,
            U64F64::from_num(1.5)
        );
        assert_err!(
            open(3, MINT, COLLATERAL),
            Error::<Test>::DissolutionInProgress
        );
        // Settling at EMA ignores a later spot move.
        set_pool(POOL * 4, POOL);

        let step = <Test as Config>::WeightInfo::settle_dissolution();
        let mut calls = 0;
        loop {
            calls += 1;
            let mut meter = WeightMeter::with_limit(step);
            if Lending::settle_positions_on_dissolution(netuid(), &mut meter) {
                break;
            }
        }
        assert_eq!(calls, 3, "one position per call, then the reserve");
        write(b"test/dissolved", true);
        let owed = MINT * 3 / 2;
        assert_eq!(
            pot(),
            2 * owed + 7,
            "I8: pot gains owed EMA value plus reserve"
        );
        assert_eq!(tao(&account(1)), funds[0] - owed);
        assert_eq!(tao(&account(2)), funds[1] - owed);
        let settled = events()
            .iter()
            .filter(|e| matches!(e, Event::ShortSettledOnDissolution { .. }))
            .count();
        assert_eq!(settled, 2, "I8: each position settles exactly once");
        assert_eq!(SyntheticOutstanding::<Test>::get(netuid()), 0);
        assert_eq!(ResidualAlpha::<Test>::get(netuid()), 0);
        assert_ok!(Lending::finish_dissolution(netuid()));
        assert_eq!(TotalPositions::<Test>::get(), 0);
    });
}

#[test]
fn dissolution_shortfall_is_not_chased() {
    run(|| {
        assert_ok!(open(1, MINT, COLLATERAL));
        let funds = position(1).funds();
        set_ema(10.0);
        assert_ok!(Lending::start_dissolution(netuid()));
        let mut meter = WeightMeter::new();
        assert!(Lending::settle_positions_on_dissolution(
            netuid(),
            &mut meter
        ));
        write(b"test/dissolved", true);
        assert_eq!(pot(), funds);
        assert_eq!(tao(&account(1)), 0);
    });
}

// ---- gates, root, coldkey swap -------------------------------------------------------

#[test]
fn disabled_blocks_growth_only() {
    run(|| {
        assert_ok!(open(1, MINT, COLLATERAL));
        assert_ok!(Lending::set_enabled(RuntimeOrigin::root(), false));
        assert_err!(open(2, MINT, COLLATERAL), Error::<Test>::Disabled);
        mint_tao(&account(1), COLLATERAL);
        assert_noop!(
            Lending::increase_short(RuntimeOrigin::signed(account(1)), netuid(), MINT, 0, 0),
            Error::<Test>::Disabled
        );
        assert_ok!(Lending::add_collateral(
            RuntimeOrigin::signed(account(1)),
            netuid(),
            1
        ));
        assert_ok!(Lending::poke(
            RuntimeOrigin::signed(account(9)),
            account(1),
            netuid()
        ));
        assert_ok!(Lending::close_short(
            RuntimeOrigin::signed(account(1)),
            netuid(),
            MINT,
            u64::MAX
        ));
    });
}

#[test]
fn dead_subnet_is_not_eligible() {
    run(|| {
        set_live(false);
        assert_err!(open(1, MINT, COLLATERAL), Error::<Test>::SubnetNotEligible);
    });
}

#[test]
fn root_params_are_bounded() {
    run(|| {
        assert_noop!(
            Lending::set_synth_params(RuntimeOrigin::signed(account(1)), SynthParams::default()),
            DispatchError::BadOrigin
        );
        assert_noop!(
            Lending::set_enabled(RuntimeOrigin::signed(account(1)), false),
            DispatchError::BadOrigin
        );
        let bad = [
            SynthParams {
                stress_bps: MIN_STRESS_BPS - 1,
                ..Default::default()
            },
            SynthParams {
                stress_bps: MAX_STRESS_BPS + 1,
                ..Default::default()
            },
            SynthParams {
                apr_floor_bps: 10_001,
                ..Default::default()
            },
            SynthParams {
                apr_ceil_bps: MAX_APR_BPS + 1,
                ..Default::default()
            },
            SynthParams {
                cap_bps: 0,
                ..Default::default()
            },
            SynthParams {
                twap_samples: MAX_TWAP_SAMPLES + 1,
                ..Default::default()
            },
            SynthParams {
                open_impact_bps: 10_000,
                ..Default::default()
            },
            SynthParams {
                min_notional: 0,
                ..Default::default()
            },
        ];
        for params in bad {
            assert_noop!(
                Lending::set_synth_params(RuntimeOrigin::root(), params),
                Error::<Test>::InvalidParams
            );
        }
        let ok = SynthParams {
            stress_bps: MIN_STRESS_BPS,
            ..Default::default()
        };
        assert_ok!(Lending::set_synth_params(RuntimeOrigin::root(), ok));
        assert_eq!(params(), ok);
    });
}

#[test]
fn defaults_match_decisions() {
    let p = SynthParams::default();
    assert_eq!(p.stress_bps, 30_000);
    assert_eq!(
        (p.apr_floor_bps, p.apr_ceil_bps, p.knee_bps),
        (2_000, 10_000, 2_000)
    );
    assert_eq!((p.reserve_share_bps, p.reserve_cap_bps), (5_000, 200));
    assert_eq!(
        p.twap_samples * p.twap_interval_blocks,
        21_600,
        "3-day TWAP"
    );
    assert_eq!(p.pace_bps_per_block, 5);
    assert_eq!(p.open_impact_bps, 200);
    assert_eq!(p.cap_bps, 2_000);
    assert_eq!(p.min_notional, 10_000_000_000);
    assert_eq!(p.dereg_ma_blocks, dereg::DEFAULT_DEREG_MA_BLOCKS);
    assert!(p.is_valid());
}

#[test]
fn coldkey_swap_rekeys_positions() {
    run(|| {
        assert_ok!(open(1, MINT, COLLATERAL));
        let custody = position(1).custody;
        assert_ok!(Lending::on_coldkey_swap(&account(1), &account(2)));
        assert!(Positions::<Test>::get(account(1), netuid()).is_none());
        assert_eq!(position(2).custody, custody);
        assert_eq!(
            EscrowOwner::<Test>::get(netuid(), &custody),
            Some(account(2))
        );
        check_invariants();
        assert_ok!(Lending::close_short(
            RuntimeOrigin::signed(account(2)),
            netuid(),
            MINT,
            u64::MAX
        ));
        assert!(tao(&account(2)) > 0);
        assert_eq!(tao(&account(1)), 0);
    });
}

#[test]
fn interface_reports_synthetic_and_residual() {
    run(|| {
        assert_ok!(open(1, MINT, COLLATERAL));
        let (s, r) = <Lending as LendingInterface<Account>>::synthetic_and_residual(netuid());
        assert_eq!((s, r), (MINT, 0));
        assert_eq!(
            <Lending as LendingInterface<Account>>::emission_reserve_offsets(netuid()),
            (0, 0, 0, 0, 0)
        );
        let (_, hot) = <Lending as LendingInterface<Account>>::custody_accounts(netuid()).unwrap();
        assert!(<Lending as LendingInterface<Account>>::is_lending_custody(
            &hot,
            &position(1).custody,
            netuid()
        ));
    });
}

#[test]
fn health_and_close_quotes() {
    run(|| {
        assert_ok!(open(1, MINT, COLLATERAL));
        let health = Lending::short_health(&account(1), netuid()).unwrap();
        assert_eq!(health.minted, MINT);
        assert!(health.equity_twap > 0);
        assert!(!health.deleveraging);
        let quote = Lending::quote_close_short(&account(1), netuid(), MINT).unwrap();
        assert_ok!(Lending::close_short(
            RuntimeOrigin::signed(account(1)),
            netuid(),
            MINT,
            quote.tao_in
        ));
        assert_eq!(tao(&account(1)), quote.refund);
    });
}

// ---- migration and dereg ----------------------------------------------------------

#[test]
fn migration_clears_lift_storage_and_installs_defaults() {
    run(|| {
        let pallet = b"Lending";
        for item in ["Lifts", "LiftParamsStore", "Positions", "Expiry"] {
            let mut k = frame_support::storage::storage_prefix(pallet, item.as_bytes()).to_vec();
            k.extend_from_slice(&[1, 2, 3]);
            sp_io::storage::set(&k, &[9, 9]);
        }
        assert!(crate::migrations::retired_keys_left::<Test>() >= 4);
        Enabled::<Test>::put(true);
        SynthParamsStore::<Test>::mutate(|p| p.stress_bps = MIN_STRESS_BPS);
        assert_ok!(Lending::initialize_custody());
        let hotkey = PalletHotkey::<Test>::get();
        crate::migrations::migrate_to_synth_shorts_v1::<Test>();
        assert_eq!(crate::migrations::retired_keys_left::<Test>(), 0);
        assert!(!Enabled::<Test>::get());
        assert_eq!(params(), SynthParams::default());
        assert_eq!(PalletHotkey::<Test>::get(), hotkey);
        // Idempotent: a second run leaves root's later choices alone.
        Enabled::<Test>::put(true);
        crate::migrations::migrate_to_synth_shorts_v1::<Test>();
        assert!(Enabled::<Test>::get());
    });
}

#[test]
fn dereg_moving_price_seeds_at_spot_tracks_slowly_and_resets_on_registration() {
    run(|| {
        assert_eq!(params().dereg_ma_blocks, dereg::DEFAULT_DEREG_MA_BLOCKS);
        let seed = MockPool::current_alpha_price(netuid());
        assert_eq!(Lending::dereg_moving_price(netuid()), None);
        <Lending as LendingInterface<_>>::update_dereg_moving_price(netuid());
        assert_eq!(Lending::dereg_moving_price(netuid()), Some(seed));

        // A short dump halves spot; one block moves the rank price 1/window of the gap.
        SynthParamsStore::<Test>::mutate(|p| p.dereg_ma_blocks = 1_000);
        set_pool(POOL, 2 * POOL);
        let dumped = MockPool::current_alpha_price(netuid());
        <Lending as LendingInterface<_>>::update_dereg_moving_price(netuid());
        let after = Lending::dereg_moving_price(netuid()).unwrap_or_default();
        assert_eq!(after, dereg::dereg_ema_step(Some(seed), dumped, 1_000));
        assert!(after < seed && after > dumped);

        <Lending as LendingInterface<_>>::on_subnet_registered(netuid());
        assert_eq!(Lending::dereg_moving_price(netuid()), None);
        set_pool(POOL, POOL);
    });
}
