#![allow(
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing
)]

//! Focused tests for synthetic-mint shorts (SPEC v1.2 §6).

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
const DEPOSIT: u64 = 100_000_000;
const HALF_WEIGHT: u64 = 500_000_000_000_000_000;

fn account(n: u8) -> Account {
    Account::new([n; 32])
}
fn burn_account() -> Account {
    account(255)
}
fn pot_account() -> Account {
    account(254)
}
fn netuid() -> NetUid {
    64.into()
}
/// A second live subnet whose rank price sets the prune-zone floor.
fn other() -> NetUid {
    65.into()
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
    // Keep the uncapped notional EMA in sync with the test feed.
    NotionalEma::<Test>::insert(netuid(), U64F64::from_num(price));
}
fn set_live(live: bool) {
    write(&key(b"test/dead", netuid()), !live);
}
fn spot() -> f64 {
    MockPool::current_alpha_price(netuid()).to_num::<f64>()
}
/// Residual alpha `r`, with issuance kept at base + synthetic + residual (I2).
fn set_residual(r: u64) {
    ResidualAlpha::<Test>::insert(netuid(), r);
    write(
        &key(b"test/issuance", netuid()),
        base_issuance() + SyntheticOutstanding::<Test>::get(netuid()) + r,
    );
}
fn set_other_rank(price: f64) {
    RankPrice::<Test>::insert(other(), U64F64::from_num(price));
    RankRingGeneration::<Test>::insert(other(), RankGeneration::<Test>::get());
    // Rank mutations in tests are mid-block; production refreshes the floor
    // only in on_initialize, so republish here.
    Lending::refresh_prune_floor_for_test();
}

fn seed_twap_samples(netuid: NetUid, samples: u32, price: f64) {
    let mut ring = crate::TwapState::<u64>::default();
    for _ in 0..samples {
        let _ = ring.samples.try_push(U64F64::from_num(price));
    }
    TwapRing::<Test>::insert(netuid, ring);
}

fn seed_rank_from_ema(netuid: NetUid, price: f64) {
    let params = params();
    let mut state = dereg::RankState::default();
    dereg::seed_rank_ring(&mut state, U64F64::from_num(price), params.rank_days);
    RankRing::<Test>::insert(netuid, state);
    // Simulate a fully-warmed ring: publish the first SMA so rank_price() is non-zero.
    RankPrice::<Test>::insert(netuid, U64F64::from_num(price));
    RankRingGeneration::<Test>::insert(netuid, RankGeneration::<Test>::get());
    Lending::refresh_prune_floor_for_test();
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
    fn notional_alpha_price(netuid: NetUid) -> U64F64 {
        Self::moving_alpha_price(netuid)
    }
    fn live_subnets() -> Vec<NetUid> {
        vec![netuid(), other()]
    }
    fn subnet_limit() -> u64 {
        128
    }
    fn network_registered_at(netuid: NetUid) -> u64 {
        // Stable ordering for floor/target ties in unit tests.
        u64::from(u16::from(netuid))
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
    ) -> Result<(TaoBalance, AlphaBalance), DispatchError> {
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
        Ok((t.into(), bought.into()))
    }
    fn burn_interest_tao(account: &Account, amount: TaoBalance) -> DispatchResult {
        if read::<bool>(b"test/fail_burn") {
            return Err("forced burn fail".into());
        }
        Self::transfer_tao(account, &burn_account(), amount)
    }
    fn subnet_pot_account(_: NetUid) -> Option<Account> {
        Some(pot_account())
    }
    fn credit_pot_reserve(netuid: NetUid, amount: TaoBalance) {
        write(&key(b"test/pot", netuid), pot() + amount.to_u64());
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
    fn free_tao(who: &Account) -> TaoBalance {
        tao(who).into()
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
        seed_twap_samples(netuid(), MIN_HEALTH_TWAP_SAMPLES, 1.0);
        seed_rank_from_ema(netuid(), 1.0);
        // Far below netuid 64, so 64 sits outside the prune zone by default.
        set_other_rank(0.1);
        seed_rank_from_ema(other(), 0.1);
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

/// I1 and I11 via `do_try_state`, I2 issuance reconciliation, I3 custody and
/// reserve balances.
fn check_invariants() {
    Lending::on_finalize(System::block_number());
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
    for (_, _, pos) in Positions::<Test>::iter() {
        assert_eq!(
            tao(&pos.custody),
            pos.funds() + pos.deposit,
            "I3: custody TAO"
        );
    }
    for (n, r) in ResidualReserve::<Test>::iter() {
        assert_eq!(tao(&Lending::reserve_account(n)), r, "I3: reserve TAO");
    }
}

fn params() -> SynthParams {
    SynthParamsStore::<Test>::get()
}

/// Fund `who` with `collateral` plus the deposit and open with the minimum
/// non-zero slippage bound.
fn open(who: u8, mint: u64, collateral: u64) -> DispatchResult {
    let owner = account(who);
    mint_tao(&owner, collateral + DEPOSIT);
    Lending::open_short(RuntimeOrigin::signed(owner), netuid(), mint, collateral, 1)
}

fn close(who: u8, alpha: u64) -> DispatchResult {
    Lending::close_short(
        RuntimeOrigin::signed(account(who)),
        netuid(),
        alpha,
        u64::MAX,
    )
}

fn position(who: u8) -> ShortPosition<Account, u64> {
    Positions::<Test>::get(account(who), netuid()).expect("position")
}

fn run_to(block: u64) {
    while System::block_number() < block {
        Lending::on_finalize(System::block_number());
        let next = System::block_number() + 1;
        System::set_block_number(next);
        Lending::on_initialize(next);
        Lending::on_idle(next, Weight::MAX);
    }
}

fn step_block() {
    Lending::on_finalize(System::block_number());
    let next = System::block_number() + 1;
    System::set_block_number(next);
    Lending::on_initialize(next);
}

/// Advance the block number without the full hook path, but keep
/// [`PruneFloor`] coherent so zone checks stay current when tests skip the full hook path.
fn set_block(n: u64) {
    System::set_block_number(n);
    if Enabled::<Test>::get() {
        Lending::refresh_prune_floor_for_test();
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

/// Put `who` into deleverage the way an unhealthy poke does.
fn force_deleverage(who: u8) {
    let since = System::block_number();
    let mut pos = position(who);
    pos.state = ShortState::Deleveraging { since };
    Positions::<Test>::insert(account(who), netuid(), &pos);
    let seq = NextDeleverageSeq::<Test>::mutate(netuid(), |s| {
        *s = s.saturating_add(1);
        *s
    });
    DeleverageQueue::<Test>::insert(netuid(), account(who), seq);
    DeleveragingMinted::<Test>::mutate(netuid(), |d| *d += pos.minted);
}

/// Fill the TWAP ring at the current spot.
fn fill_twap(samples: u32) {
    SynthParamsStore::<Test>::mutate(|p| {
        p.twap_samples = samples;
        p.twap_interval_blocks = 10;
    });
    let start = System::block_number();
    for i in 1..=u64::from(samples) {
        run_to((start / 10 + i) * 10);
    }
    assert_eq!(
        TwapRing::<Test>::get(netuid()).samples.len(),
        samples as usize
    );
}

// ---- open / close ----------------------------------------------------------

#[test]
fn open_close_round_trip_costs_only_rounding_and_refunds_deposit() {
    run(|| {
        let (y0, x0) = pool();
        assert_ok!(open(1, MINT, COLLATERAL));
        let pos = position(1);
        assert_eq!(pos.minted, MINT);
        assert_eq!(pos.collateral, COLLATERAL);
        assert_eq!(pos.deposit, DEPOSIT);
        assert!(pos.proceeds > 0);
        assert_eq!(SyntheticOutstanding::<Test>::get(netuid()), MINT);
        assert_eq!(issuance(), ISSUANCE + MINT);
        assert_eq!(tao(&account(1)), 0);
        check_invariants();

        assert_ok!(close(1, MINT));
        assert!(Positions::<Test>::get(account(1), netuid()).is_none());
        assert_eq!(SyntheticOutstanding::<Test>::get(netuid()), 0);
        assert_eq!(issuance(), ISSUANCE);
        assert_eq!(burned_alpha(), MINT);
        let refund = tao(&account(1));
        let lost = COLLATERAL + DEPOSIT - refund;
        assert!(lost <= 2, "round trip lost {lost}");
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
        set_block(11);
        assert_ok!(close(1, MINT / 2));
        assert_eq!(position(1).minted, MINT / 2);
        check_invariants();

        mint_tao(&account(1), COLLATERAL);
        assert_ok!(Lending::increase_short(
            RuntimeOrigin::signed(account(1)),
            netuid(),
            MINT / 2,
            COLLATERAL,
            1
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

#[test]
fn slippage_bound_is_mandatory_and_enforced() {
    run(|| {
        let quote = Lending::quote_open_short(netuid(), MINT).unwrap();
        mint_tao(&account(1), COLLATERAL + DEPOSIT);
        for min_tao_out in [0, quote.proceeds + 1] {
            assert_noop!(
                Lending::open_short(
                    RuntimeOrigin::signed(account(1)),
                    netuid(),
                    MINT,
                    COLLATERAL,
                    min_tao_out
                ),
                Error::<Test>::SlippageExceeded
            );
        }
        assert_ok!(Lending::open_short(
            RuntimeOrigin::signed(account(1)),
            netuid(),
            MINT,
            COLLATERAL,
            quote.proceeds
        ));
        mint_tao(&account(1), COLLATERAL);
        assert_noop!(
            Lending::increase_short(
                RuntimeOrigin::signed(account(1)),
                netuid(),
                MINT,
                COLLATERAL,
                0
            ),
            Error::<Test>::SlippageExceeded
        );
    });
}

// ---- coverage (P2) ---------------------------------------------------------

#[test]
fn quote_required_collateral_is_the_coverage_boundary() {
    run(|| {
        let quote = Lending::quote_open_short(netuid(), MINT).unwrap();
        assert_eq!(quote.notional, MINT);
        assert_eq!(quote.deposit, DEPOSIT);
        assert!(!quote.in_prune_zone);
        assert_err!(
            open(1, MINT, quote.required_collateral - 1),
            Error::<Test>::CoverageNotMet
        );
        assert_ok!(open(1, MINT, quote.required_collateral));
        let pos = position(1);
        assert_eq!(pos.proceeds, quote.proceeds);
        let cost = Lending::coverage_cost(netuid(), MINT, &params()).unwrap();
        assert_eq!(pos.funds(), cost);
        // About 3× notional at the default stress, a little more for the book.
        assert!(cost > 3 * MINT && cost < 31 * MINT / 10, "{cost}");
    });
}

#[test]
fn stress_eff_scales_with_book_over_pool_alpha() {
    let p = SynthParams::default();
    assert_eq!(Lending::stress_eff_bps(0, POOL, &p), 30_000);
    // 3 × (1 + 2 × ½) = 6.
    assert_eq!(Lending::stress_eff_bps(POOL / 2, POOL, &p), 60_000);
    // 3 × (1 + 2 × 1%) = 3.06.
    assert_eq!(Lending::stress_eff_bps(POOL / 100, POOL, &p), 30_600);
    let flat = SynthParams {
        stress_slope_bps: 0,
        ..p
    };
    assert_eq!(Lending::stress_eff_bps(POOL, POOL, &flat), 30_000);
    assert_eq!(Lending::stress_eff_bps(1, 0, &p), u128::MAX);
}

#[test]
fn whole_book_coverage_is_pro_rata_and_grows_with_the_book() {
    run(|| {
        assert_ok!(open(1, MINT, COLLATERAL));
        let alone = Lending::coverage_cost(netuid(), MINT, &params()).unwrap();
        set_block(11);
        assert_ok!(open(2, 5 * MINT, 5 * COLLATERAL));
        // The earlier short now shares a larger book with a steeper stress.
        let shared = Lending::coverage_cost(netuid(), MINT, &params()).unwrap();
        assert!(shared > alone, "{shared} <= {alone}");
        let big = Lending::coverage_cost(netuid(), 5 * MINT, &params()).unwrap();
        assert!(
            big.abs_diff(5 * shared) <= 5,
            "pro-rata: {big} vs 5 × {shared}"
        );
        // A later quote prices the whole book, not its place in a queue.
        let quote = Lending::quote_open_short(netuid(), MINT).unwrap();
        assert!(quote.required_collateral + quote.proceeds > shared);
    });
}

#[test]
fn unbuyable_book_is_coverage_not_met() {
    run(|| {
        assert_ok!(open(1, MINT, COLLATERAL));
        set_block(11);
        // A pool this small cannot supply the book after the stress pump.
        set_pool(MINT, MINT);
        assert_eq!(Lending::coverage_cost(netuid(), MINT, &params()), None);
        assert_err!(
            Lending::quote_open_short(netuid(), params().min_notional),
            Error::<Test>::CoverageNotMet
        );
        mint_tao(&account(1), COLLATERAL);
        assert_noop!(
            Lending::increase_short(
                RuntimeOrigin::signed(account(1)),
                netuid(),
                0,
                COLLATERAL,
                0
            ),
            Error::<Test>::CoverageNotMet
        );
        set_pool(POOL, POOL);
    });
}

#[test]
fn coverage_reference_is_max_of_ema_spot_and_rank_sma() {
    run(|| {
        let at_spot = Lending::quote_open_short(netuid(), MINT).unwrap();
        set_ema(2.0);
        let at_ema = Lending::quote_open_short(netuid(), MINT).unwrap();
        assert!(at_ema.required_collateral > at_spot.required_collateral * 2);
        set_ema(1.0);
        RankPrice::<Test>::insert(netuid(), U64F64::from_num(2.0));
        RankRingGeneration::<Test>::insert(netuid(), RankGeneration::<Test>::get());
        let at_rank = Lending::quote_open_short(netuid(), MINT).unwrap();
        assert_eq!(at_rank.required_collateral, at_ema.required_collateral);
    });
}

// ---- withdraw (P20) ----------------------------------------------------------

#[test]
fn withdraw_keeps_coverage_plus_interest_buffer() {
    run(|| {
        assert_ok!(open(1, MINT, COLLATERAL));
        let pos = position(1);
        let p = params();
        let cost = Lending::coverage_cost(netuid(), MINT, &p).unwrap();
        let buffer = Lending::days_interest(netuid(), MINT, 7, &p);
        assert!(buffer > 0);
        let slack = pos.funds() - cost;
        let withdraw =
            |tao| Lending::withdraw_collateral(RuntimeOrigin::signed(account(1)), netuid(), tao);
        assert_noop!(withdraw(slack + 1), Error::<Test>::WithdrawBreaksCoverage);
        assert_noop!(
            withdraw(slack - buffer + 1),
            Error::<Test>::WithdrawBufferBreached
        );
        assert_ok!(withdraw(slack - buffer));
        assert_eq!(tao(&account(1)), slack - buffer);
    });
}

// ---- minimum, depth, net mint (P8, P10, P18) -------------------------------------

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
        assert_noop!(close(1, 1), Error::<Test>::BelowMinNotional);
        set_ema(0.5);
        assert_err!(open(2, min, COLLATERAL), Error::<Test>::BelowMinNotional);
    });
}

#[test]
fn net_mint_per_block_spans_accounts_and_nets_burns() {
    run(|| {
        // Limit is 1% of pool alpha: about 1e13 here.
        let big = 6 * MINT;
        assert_ok!(open(1, big, 4 * big));
        assert_err!(open(2, big, 4 * big), Error::<Test>::OpenImpactTooHigh);
        assert_err!(
            Lending::quote_open_short(netuid(), big),
            Error::<Test>::OpenImpactTooHigh
        );
        // Buying back in the same block frees room.
        assert_ok!(close(1, big / 2));
        assert_eq!(Lending::block_net_minted(netuid()), i128::from(big / 2));
        assert_ok!(open(2, big, 4 * big));
        Lending::on_finalize(System::block_number());
        assert_eq!(BlockNetMinted::<Test>::iter().count(), 0);
        set_block(11);
        assert_eq!(Lending::block_net_minted(netuid()), 0);
        assert_ok!(open(3, big, 4 * big));
    });
}

#[test]
fn pool_depth_floor_blocks_growth() {
    run(|| {
        assert_eq!(params().min_pool_depth, 0);
        SynthParamsStore::<Test>::mutate(|p| p.min_pool_depth = POOL + 1);
        assert_err!(open(1, MINT, COLLATERAL), Error::<Test>::PoolTooShallow);
        assert_err!(
            Lending::quote_open_short(netuid(), MINT),
            Error::<Test>::PoolTooShallow
        );
        SynthParamsStore::<Test>::mutate(|p| p.min_pool_depth = POOL);
        assert_ok!(open(1, MINT, COLLATERAL));
    });
}

// ---- dilution breaker (P21) ----------------------------------------------------------

#[test]
fn dilution_breaker_trips_above_two_percent_and_clears_below_one() {
    run(|| {
        assert_ok!(open(1, MINT, COLLATERAL));
        let ex = ISSUANCE;
        let p = params();
        set_residual(ex * 200 / 10_000);
        Lending::refresh_breaker(netuid(), &p);
        assert!(
            !BreakerTripped::<Test>::get(netuid()),
            "2% exactly stays open"
        );
        set_residual(ex * 201 / 10_000);
        Lending::refresh_breaker(netuid(), &p);
        assert!(BreakerTripped::<Test>::get(netuid()));
        assert!(events().contains(&Event::DilutionBreaker {
            netuid: netuid(),
            tripped: true
        }));
        set_block(11);
        assert_err!(
            open(2, MINT, COLLATERAL),
            Error::<Test>::DilutionBreakerActive
        );
        assert_noop!(
            Lending::increase_short(RuntimeOrigin::signed(account(1)), netuid(), MINT, 0, 1),
            Error::<Test>::DilutionBreakerActive
        );
        assert_err!(
            Lending::quote_open_short(netuid(), MINT),
            Error::<Test>::DilutionBreakerActive
        );
        // Hysteresis: 1.5% keeps it latched.
        set_residual(ex * 150 / 10_000);
        assert_err!(
            open(2, MINT, COLLATERAL),
            Error::<Test>::DilutionBreakerActive
        );
        // Closing and adding collateral stay open while latched.
        mint_tao(&account(1), 1);
        assert_ok!(Lending::add_collateral(
            RuntimeOrigin::signed(account(1)),
            netuid(),
            1
        ));
        set_residual(ex * 99 / 10_000);
        assert_ok!(open(2, MINT, COLLATERAL));
        assert!(!BreakerTripped::<Test>::get(netuid()));
        assert!(events().contains(&Event::DilutionBreaker {
            netuid: netuid(),
            tripped: false
        }));
        assert_ok!(close(1, MINT));
    });
}

// ---- interest (P3, P4) ------------------------------------------------------------

fn expected_interest(minted: u64, blocks: u64, remainder: u64) -> (u64, u64) {
    let notional = u128::from(minted);
    let apr = u128::from(params().apr_bps);
    let num = notional * apr * u128::from(blocks) + u128::from(remainder);
    let den = 10_000u128 * u128::from(BLOCKS_PER_YEAR);
    ((num / den) as u64, (num % den) as u64)
}

#[test]
fn apr_is_flat_regardless_of_book() {
    run(|| {
        let before = Lending::quote_open_short(netuid(), MINT).unwrap();
        assert_eq!(before.apr_bps, 2_000);
        assert_eq!(before.annual_interest, MINT / 5);
        assert_ok!(open(1, 9 * MINT, 9 * COLLATERAL));
        set_block(11);
        let after = Lending::quote_open_short(netuid(), MINT).unwrap();
        assert_eq!(after.apr_bps, 2_000);
        assert_eq!(after.annual_interest, MINT / 5);
    });
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
fn exhausted_collateral_starts_deleverage() {
    run(|| {
        assert_ok!(open(1, MINT, COLLATERAL));
        SynthParamsStore::<Test>::mutate(|p| p.apr_bps = MAX_APR_BPS);
        // 100% APR on 1e12 for 1000 days far exceeds the collateral.
        set_block(100_000);
        Lending::on_idle(100_000, Weight::MAX);
        let pos = position(1);
        assert_eq!(pos.collateral, 0, "I5: charge capped at collateral");
        assert!(matches!(pos.state, ShortState::Deleveraging { .. }));
        assert!(DeleverageQueue::<Test>::contains_key(netuid(), account(1)));
        assert_eq!(DeleveragingMinted::<Test>::get(netuid()), MINT);
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

// ---- TWAP (P6) -------------------------------------------------------------------

#[test]
fn twap_is_the_ring_median_and_wraps() {
    run(|| {
        SynthParamsStore::<Test>::mutate(|p| {
            p.twap_samples = 3;
            p.twap_interval_blocks = 10;
        });
        assert_eq!(Lending::twap(netuid()).to_num::<f64>(), 1.0);
        for (block, price) in [(20u64, 1.0f64), (30, 2.0), (40, 4.0)] {
            set_pool((POOL as f64 * price) as u64, POOL);
            run_to(block);
        }
        assert_eq!(TwapRing::<Test>::get(netuid()).samples.len(), 3);
        assert_eq!(Lending::twap(netuid()).to_num::<f64>(), 2.0);
        // Overwrites the oldest (1.0): [10, 2, 4] has median 4.
        set_pool(POOL * 10, POOL);
        run_to(50);
        assert_eq!(Lending::twap(netuid()).to_num::<f64>(), 4.0);
        assert_eq!(TwapRing::<Test>::get(netuid()).cursor, 1);
        // An even count averages the middle pair.
        SynthParamsStore::<Test>::mutate(|p| p.twap_samples = 4);
        set_pool(POOL * 3, POOL);
        run_to(60);
        assert_eq!(Lending::twap(netuid()).to_num::<f64>(), 3.5);
        // A re-registered netuid starts clean.
        <Lending as LendingInterface<Account>>::on_subnet_registered(netuid());
        assert!(TwapRing::<Test>::get(netuid()).samples.is_empty());
        set_pool(POOL, POOL);
    });
}

#[test]
fn one_block_145x_spike_leaves_the_median_unchanged() {
    run(|| {
        fill_twap(72);
        let before = Lending::twap(netuid());
        assert_eq!(before.to_num::<f64>(), 1.0);
        // One block at 145× spot mid-interval must not move the interval mean enough to flip the median.
        set_pool(POOL * 145, POOL);
        step_block();
        set_pool(POOL, POOL);
        let next = (System::block_number() / 10 + 1) * 10;
        run_to(next);
        assert_eq!(Lending::twap(netuid()), before);
    });
}

#[test]
fn interval_mean_softens_a_spike_on_the_sample_boundary() {
    run(|| {
        fill_twap(72);
        let before = Lending::twap(netuid());
        SynthParamsStore::<Test>::mutate(|p| p.twap_interval_blocks = 10);
        set_pool(POOL * 145, POOL);
        let boundary = (System::block_number() / 10 + 1) * 10;
        run_to(boundary - 1);
        set_pool(POOL, POOL);
        step_block();
        assert_eq!(Lending::twap(netuid()), before);
    });
}

#[test]
fn twap_smooths_a_spike_out_of_equity() {
    run(|| {
        assert_ok!(open(1, MINT, COLLATERAL));
        fill_twap(72);
        // Spot spikes 4×; the median still sits near 1, so equity stays positive.
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

// ---- deleverage (P5, P7, P17, P19) -------------------------------------------------

fn pump_and_deleverage() {
    assert_ok!(open(1, MINT, COLLATERAL));
    // Spot 5×: buying 1e12 back costs about 5e12 > funds.
    let (y, x) = pool();
    set_pool(y * 5, x);
    seed_twap_samples(netuid(), MIN_HEALTH_TWAP_SAMPLES, spot());
    assert_ok!(Lending::poke(
        RuntimeOrigin::signed(account(9)),
        account(1),
        netuid()
    ));
    assert!(matches!(position(1).state, ShortState::Deleveraging { .. }));
    assert_eq!(DeleveragingMinted::<Test>::get(netuid()), MINT);
    assert!(matches!(
        events().last(),
        Some(Event::DeleverageStarted { equity_twap, .. }) if *equity_twap < 0
    ));
}

#[test]
fn repeated_poke_in_one_block_cannot_exceed_locked_share() {
    run(|| {
        assert_ok!(open(1, MINT, COLLATERAL));
        let (y, x) = pool();
        set_pool(y * 5, x);
        seed_twap_samples(netuid(), MIN_HEALTH_TWAP_SAMPLES, spot());
        force_deleverage(1);
        let owner = account(1);
        let n = netuid();
        for _ in 0..8 {
            assert_ok!(Lending::poke(
                RuntimeOrigin::signed(account(9)),
                owner.clone(),
                n,
            ));
        }
        let (at, alloc, spent) =
            DeleverageFilled::<Test>::get(n, &owner).expect("deleverage fill map");
        assert_eq!(at, System::block_number());
        assert!(spent <= alloc, "spent {spent} exceeds locked share {alloc}");
    });
}

#[test]
fn late_join_deleverage_waits_until_next_block_cohort() {
    run(|| {
        assert_ok!(open(1, MINT, COLLATERAL));
        assert_ok!(open(2, MINT, COLLATERAL));
        force_deleverage(1);
        let params = params();
        // Lock this block's budget/cutoff on position 1 only.
        assert!(Lending::deleverage_step(&account(1), netuid(), &params));
        let (_, _, _, cutoff) = DeleverageBudget::<Test>::get(netuid()).expect("budget locked");
        let seq1 = DeleverageQueue::<Test>::get(netuid(), account(1)).expect("queued");
        assert!(seq1 <= cutoff);
        force_deleverage(2);
        let seq2 = DeleverageQueue::<Test>::get(netuid(), account(2)).expect("queued");
        assert!(seq2 > cutoff, "late join must be past the locked cutoff");
        let minted_before = position(2).minted;
        assert!(!Lending::deleverage_step(&account(2), netuid(), &params));
        assert_eq!(position(2).minted, minted_before, "late join must not fill");
        step_block();
        // Next block's cutoff includes position 2; on_initialize may already step it.
        let after = Positions::<Test>::get(account(2), netuid()).map(|p| p.minted);
        assert!(
            after.is_none() || after.unwrap() < minted_before,
            "next block must admit the late join into the cohort"
        );
    });
}

#[test]
fn deleverage_respects_pace_and_defaults_with_residual_burning_deposit() {
    run(|| {
        pump_and_deleverage();
        let pace = f64::from(params().pace_bps_per_block) / 10_000.0;
        let mut blocks = 0;
        while Positions::<Test>::contains_key(account(1), netuid()) {
            let before = spot();
            step_block();
            assert!(
                spot() <= before * (1.0 + pace) * (1.0 + 1e-12),
                "I9: pace exceeded"
            );
            check_invariants();
            blocks += 1;
            assert!(blocks < 2_000, "deleverage must finish");
        }
        assert!(blocks > 1, "deleverage must be partial across blocks");
        let residual = ResidualAlpha::<Test>::get(netuid());
        assert!(residual > 0);
        assert_eq!(SyntheticOutstanding::<Test>::get(netuid()), 0);
        assert_eq!(DeleveragingMinted::<Test>::get(netuid()), 0);
        assert_eq!(burned_alpha() + residual, MINT);
        assert_eq!(
            tao(&burn_account()),
            DEPOSIT,
            "P19: deposit burns on default"
        );
        assert_eq!(tao(&account(1)), 0);
        assert!(events().iter().any(|e| matches!(
            e,
            Event::ShortDefaulted { residual_alpha, .. } if *residual_alpha == residual
        )));
    });
}

#[test]
fn deleverage_fills_are_pro_rata_across_the_book() {
    run(|| {
        assert_ok!(open(1, MINT, COLLATERAL));
        assert_ok!(open(2, 3 * MINT, 3 * COLLATERAL));
        force_deleverage(1);
        force_deleverage(2);
        assert_eq!(DeleveragingMinted::<Test>::get(netuid()), 4 * MINT);
        let before = spot();
        step_block();
        let pace = f64::from(params().pace_bps_per_block) / 10_000.0;
        assert!(spot() <= before * (1.0 + pace) * (1.0 + 1e-12));
        let spent = |who: u8| {
            events()
                .iter()
                .find_map(|e| match e {
                    Event::DeleverageStep {
                        who: w, tao_spent, ..
                    } if *w == account(who) => Some(*tao_spent as f64),
                    _ => None,
                })
                .expect("step")
        };
        let ratio = spent(2) / spent(1);
        assert!((ratio - 3.0).abs() < 0.01, "pro-rata ratio {ratio}");
        assert_eq!(
            DeleveragingMinted::<Test>::get(netuid()),
            position(1).minted + position(2).minted
        );
    });
}

#[test]
fn buybacks_pause_while_spot_is_pumped_over_the_median() {
    run(|| {
        assert_ok!(open(1, MINT, COLLATERAL));
        fill_twap(3);
        force_deleverage(1);
        let reserve = Lending::reserve_account(netuid());
        mint_tao(&reserve, MINT);
        ResidualReserve::<Test>::mutate(netuid(), |r| *r += MINT);
        set_residual(1_000_000);
        // 1.3× the median is above the 1.25× pause line.
        set_pool(POOL * 13 / 10, POOL);
        assert!(Lending::buybacks_paused(netuid(), &params()));
        step_block();
        assert_eq!(position(1).minted, MINT);
        assert_eq!(ResidualAlpha::<Test>::get(netuid()), 1_000_000);
        assert!(!events().iter().any(|e| matches!(
            e,
            Event::DeleverageStep { .. } | Event::ResidualRetired { .. }
        )));
        set_pool(POOL * 12 / 10, POOL);
        assert!(!Lending::buybacks_paused(netuid(), &params()));
        step_block();
        assert!(position(1).minted < MINT);
        set_pool(POOL, POOL);
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
        assert_ok!(close(1, MINT));
        assert!(!DeleverageQueue::<Test>::contains_key(netuid(), account(1)));
        assert_eq!(DeleveragingMinted::<Test>::get(netuid()), 0);
    });
}

#[test]
fn reserve_retires_residual_under_pace() {
    run(|| {
        pump_and_deleverage();
        run_to(System::block_number() + 2_000);
        assert!(!Positions::<Test>::contains_key(account(1), netuid()));
        let residual = ResidualAlpha::<Test>::get(netuid());
        assert!(residual > 0);
        let reserve = Lending::reserve_account(netuid());
        let funding = 100 * MINT;
        mint_tao(&reserve, funding);
        ResidualReserve::<Test>::mutate(netuid(), |r| *r += funding);
        // Re-anchor the median at the current spot so retirement is not paused.
        TwapRing::<Test>::remove(netuid());
        let pace = f64::from(params().pace_bps_per_block) / 10_000.0;
        let before = spot();
        step_block();
        assert!(spot() <= before * (1.0 + pace) * (1.0 + 1e-12));
        let after = ResidualAlpha::<Test>::get(netuid());
        assert!(after < residual);
        check_invariants();
        for _ in 0..400 {
            TwapRing::<Test>::remove(netuid());
            step_block();
        }
        assert_eq!(ResidualAlpha::<Test>::get(netuid()), 0);
    });
}

// ---- prune zone (P16) ---------------------------------------------------------------

#[test]
fn opens_in_the_prune_zone_track_zone_tranche() {
    run(|| {
        // Floor 0.9: 1.0 ≤ 1.25 × 0.9, so netuid 64 is a prune candidate.
        set_other_rank(0.9);
        assert!(Lending::in_prune_zone(netuid(), &params()));
        assert!(
            Lending::quote_open_short(netuid(), MINT)
                .unwrap()
                .in_prune_zone
        );
        assert_ok!(open(1, MINT, COLLATERAL));
        let pos = position(1);
        assert_eq!(pos.zone_minted, MINT);
        assert_eq!(pos.zone_proceeds, pos.proceeds);

        set_other_rank(0.1);
        assert!(!Lending::in_prune_zone(netuid(), &params()));
        set_block(11);
        mint_tao(&account(1), COLLATERAL);
        assert_ok!(Lending::increase_short(
            RuntimeOrigin::signed(account(1)),
            netuid(),
            MINT,
            COLLATERAL,
            1
        ));
        let pos = position(1);
        assert_eq!((pos.minted, pos.zone_minted), (2 * MINT, MINT));
        let zone_proceeds = pos.zone_proceeds;

        // Burns shrink the zone tranche pro rata.
        assert_ok!(close(1, MINT));
        let pos = position(1);
        assert_eq!(pos.zone_minted, MINT / 2);
        assert_eq!(pos.zone_proceeds, zone_proceeds / 2);
    });
}

#[test]
fn zone_floor_is_the_lowest_rank_and_ignores_far_subnets() {
    run(|| {
        // Only one subnet below: 64 itself is the floor.
        set_other_rank(5.0);
        assert!(Lending::in_prune_zone(netuid(), &params()));
        set_other_rank(0.8);
        assert!(Lending::in_prune_zone(netuid(), &params()));
        set_other_rank(0.79);
        assert!(!Lending::in_prune_zone(netuid(), &params()));
    });
}

// ---- dissolution (P12, P16) ------------------------------------------------------------

#[test]
fn dissolution_settles_at_r_once_with_resuming_cursor() {
    run(|| {
        assert_ok!(open(1, MINT, COLLATERAL));
        assert_ok!(open(2, MINT, 2 * COLLATERAL));
        let reserve = Lending::reserve_account(netuid());
        mint_tao(&reserve, 7);
        ResidualReserve::<Test>::insert(netuid(), 7);
        set_residual(3);
        let funds: Vec<u64> = [1u8, 2].iter().map(|w| position(*w).funds()).collect();
        // T_ema below pool TAO wins the min.
        let t_ema = pool().0 / 2;
        PoolTaoEma::<Test>::insert(netuid(), t_ema);
        let rate = U64F64::from_bits(u128::from(t_ema) * (1u128 << 64) / u128::from(issuance()));
        assert_eq!(Lending::dissolution_rate(netuid()), rate);
        assert_ok!(Lending::start_dissolution(netuid()));
        assert_eq!(Dissolutions::<Test>::get(netuid()).unwrap().price, rate);
        assert_err!(
            open(3, MINT, COLLATERAL),
            Error::<Test>::DissolutionInProgress
        );
        // Settling at r ignores a later spot move.
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
        let owed = Lending::dissolution_owed(
            &ShortPosition {
                zone_minted: 0,
                zone_proceeds: 0,
                ..position_template()
            },
            rate,
        );
        let expect = (u128::from(MINT) * rate.to_bits()).div_ceil(1u128 << 64) as u64;
        assert_eq!(owed, expect);
        assert_eq!(pot(), 2 * owed + 7, "I8: pot gains owed value plus reserve");
        assert_eq!(tao(&account(1)), funds[0] - owed + DEPOSIT);
        assert_eq!(tao(&account(2)), funds[1] - owed + DEPOSIT);
        let settled = events()
            .iter()
            .filter(|e| matches!(e, Event::ShortSettledOnDissolution { .. }))
            .count();
        assert_eq!(settled, 2, "I8: each position settles exactly once");
        assert_eq!(SyntheticOutstanding::<Test>::get(netuid()), 0);
        assert_eq!(ResidualAlpha::<Test>::get(netuid()), 0);
        assert_ok!(Lending::finish_dissolution(netuid()));
        assert_eq!(TotalPositions::<Test>::get(), 0);
        assert_eq!(PoolTaoEma::<Test>::get(netuid()), None);
    });
}

fn position_template() -> ShortPosition<Account, u64> {
    ShortPosition {
        custody: account(0),
        minted: MINT,
        collateral: u64::MAX / 4,
        proceeds: 0,
        deposit: 0,
        zone_minted: 0,
        zone_proceeds: 0,
        opened_at: 0,
        last_accrued: 0,
        interest_remainder: 0,
        due: 0,
        state: ShortState::Open,
        terms_version: TERMS_VERSION,
    }
}

#[test]
fn zone_alpha_settles_at_its_proceeds_and_the_rest_at_r() {
    run(|| {
        set_other_rank(0.9);
        assert_ok!(open(1, MINT, COLLATERAL));
        set_other_rank(0.1);
        set_block(11);
        mint_tao(&account(1), COLLATERAL);
        assert_ok!(Lending::increase_short(
            RuntimeOrigin::signed(account(1)),
            netuid(),
            MINT,
            COLLATERAL,
            1
        ));
        let pos = position(1);
        let funds = pos.funds();
        let rate = Lending::dissolution_rate(netuid());
        let older = (u128::from(MINT) * rate.to_bits()).div_ceil(1u128 << 64) as u64;
        let owed = older + pos.zone_proceeds;
        assert_eq!(Lending::dissolution_owed(&pos, rate), owed);
        let reserve = ResidualReserve::<Test>::get(netuid());
        assert_ok!(Lending::start_dissolution(netuid()));
        let mut meter = WeightMeter::new();
        assert!(Lending::settle_positions_on_dissolution(
            netuid(),
            &mut meter
        ));
        write(b"test/dissolved", true);
        assert_eq!(pot(), owed + reserve);
        assert_eq!(tao(&account(1)), funds - owed + DEPOSIT);
    });
}

#[test]
fn dissolution_shortfall_is_not_chased() {
    run(|| {
        assert_ok!(open(1, MINT, COLLATERAL));
        let funds = position(1).funds();
        // Little alpha outstanding makes r large; the debt caps at the funds.
        write(&key(b"test/issuance", netuid()), MINT + 1);
        write(b"test/dissolved", true);
        assert_ok!(Lending::start_dissolution(netuid()));
        let mut meter = WeightMeter::new();
        assert!(Lending::settle_positions_on_dissolution(
            netuid(),
            &mut meter
        ));
        assert_eq!(pot(), funds);
        assert_eq!(tao(&account(1)), DEPOSIT);
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
            Lending::increase_short(RuntimeOrigin::signed(account(1)), netuid(), MINT, 0, 1),
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
        assert_ok!(close(1, MINT));
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
        let d = SynthParams::default;
        let bad = [
            SynthParams {
                stress_bps: MIN_STRESS_BPS - 1,
                ..d()
            },
            SynthParams {
                stress_bps: MAX_STRESS_BPS + 1,
                ..d()
            },
            SynthParams {
                stress_slope_bps: MAX_STRESS_SLOPE_BPS + 1,
                ..d()
            },
            SynthParams { apr_bps: 0, ..d() },
            SynthParams {
                apr_bps: MAX_APR_BPS + 1,
                ..d()
            },
            SynthParams {
                twap_samples: MAX_TWAP_SAMPLES + 1,
                ..d()
            },
            SynthParams {
                open_net_mint_bps: 0,
                ..d()
            },
            SynthParams {
                min_notional: 0,
                ..d()
            },
            SynthParams {
                rank_days: 0,
                ..d()
            },
            SynthParams {
                zone_bps: 9_999,
                ..d()
            },
            SynthParams {
                pause_bps: 9_999,
                ..d()
            },
            SynthParams {
                breaker_off_bps: 201,
                ..d()
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
            apr_bps: 5_000,
            stress_slope_bps: 0,
            ..d()
        };
        assert_ok!(Lending::set_synth_params(RuntimeOrigin::root(), ok));
        assert_eq!(params(), ok);
    });
}

#[test]
fn defaults_match_decisions() {
    let p = SynthParams::default();
    assert_eq!((p.stress_bps, p.stress_slope_bps), (30_000, 20_000));
    assert_eq!(p.apr_bps, 2_000);
    assert_eq!((p.reserve_share_bps, p.reserve_cap_bps), (5_000, 200));
    assert_eq!(p.twap_samples, 72);
    assert_eq!(
        p.twap_samples * p.twap_interval_blocks,
        21_600,
        "3-day TWAP"
    );
    assert_eq!(p.pace_bps_per_block, 5);
    assert_eq!(p.open_net_mint_bps, 100);
    assert_eq!(p.min_notional, 10_000_000_000);
    assert_eq!((p.rank_days, p.rank_day_blocks), (30, 7_200));
    assert_eq!((p.zone_bps, p.pause_bps), (12_500, 12_500));
    assert_eq!(p.min_pool_depth, 0);
    assert_eq!(p.deposit, DEPOSIT);
    assert_eq!(p.withdraw_buffer_days, 7);
    assert_eq!((p.breaker_on_bps, p.breaker_off_bps), (200, 100));
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
        assert_ok!(close(2, MINT));
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
        assert_eq!(health.zone_minted, 0);
        assert!(health.equity_twap > 0);
        assert!(health.day_interest > 0);
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

// ---- migration and dereg ranking (P13) ---------------------------------------------------

#[test]
fn migration_clears_retired_storage_and_installs_defaults() {
    run(|| {
        let pallet = b"Lending";
        for item in [
            "Lifts",
            "LiftParamsStore",
            "Expiry",
            "DeregMovingPrice",
            "OpenImpactAnchor",
        ] {
            let mut k = frame_support::storage::storage_prefix(pallet, item.as_bytes()).to_vec();
            k.extend_from_slice(&[1, 2, 3]);
            sp_io::storage::set(&k, &[9, 9]);
        }
        assert!(crate::migrations::retired_keys_left::<Test>() >= 5);
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
#[cfg(not(feature = "try-runtime"))]
fn migration_refuses_to_clear_nonempty_positions() {
    new_test_ext().execute_with(|| {
        let pallet = b"Lending";
        let mut k = frame_support::storage::storage_prefix(pallet, b"Positions").to_vec();
        k.extend_from_slice(&[1, 2, 3]);
        sp_io::storage::set(&k, &[9, 9]);
        Enabled::<Test>::put(true);
        crate::migrations::migrate_to_synth_shorts_v1::<Test>();
        // Refused: stamp not set, Positions key still present, Enabled unchanged.
        assert!(!HasMigrationRun::<Test>::get(
            crate::migrations::synth_shorts_v1_key()
        ));
        assert!(sp_io::storage::get(&k).is_some());
        assert!(Enabled::<Test>::get());
        // V2 must not clear StrandedCustody / deleverage maps either.
        crate::migrations::migrate_deleverage_value_types_v2::<Test>();
        assert!(!HasMigrationRun::<Test>::get(
            crate::migrations::deleverage_value_types_v2_key()
        ));
    });
}

#[test]
#[cfg(feature = "try-runtime")]
#[should_panic(expected = "lift-era Positions/EscrowOwner must be empty")]
fn migration_refuses_to_clear_nonempty_positions_try_runtime() {
    new_test_ext().execute_with(|| {
        let pallet = b"Lending";
        let mut k = frame_support::storage::storage_prefix(pallet, b"Positions").to_vec();
        k.extend_from_slice(&[1, 2, 3]);
        sp_io::storage::set(&k, &[9, 9]);
        crate::migrations::migrate_to_synth_shorts_v1::<Test>();
    });
}

#[test]
fn dissolution_invalidates_prune_floor_in_constant_time() {
    run(|| {
        // Floor 0.9: netuid at 1.0 is in zone (≤ 1.25 × 0.9).
        set_other_rank(0.9);
        assert!(Lending::in_prune_zone(netuid(), &params()));
        let before = PruneFloor::<Test>::get().expect("floor").2;
        assert_eq!(before, U64F64::from_num(0.9));
        // O(1) invalidate — no uncharged full-subnet scan on the
        // registration → dissolve path. Fail-closed until next refresh.
        assert_ok!(Lending::start_dissolution(other()));
        assert!(
            PruneFloor::<Test>::get().is_none(),
            "dissolution must kill the cached floor"
        );
        assert!(
            PruneTarget::<Test>::get().is_none(),
            "dissolution must kill the cached prune target"
        );
        assert!(
            Lending::in_prune_zone(netuid(), &params()),
            "missing floor fails closed (in zone)"
        );
        // Next metered refresh excludes the dissolving net.
        Lending::refresh_prune_floor_for_test();
        let (_at, epoch, floor) = PruneFloor::<Test>::get().expect("floor");
        assert_eq!(epoch, RankGeneration::<Test>::get());
        assert_eq!(
            floor,
            Lending::prune_rank_price(netuid()),
            "dissolving peer must leave the prune floor"
        );
        assert!(floor > before);
    });
}

#[test]
fn prune_floor_refresh_weight_charges_trie_witness_not_64b_guess() {
    // Warm-miss path: 7 reads/subnet + shared. Proof must cover MaxEncodedLen
    // of the real keys plus trie-path overhead — far above a 64 B guess.
    let n = 4u64;
    let w = Lending::prune_floor_refresh_weight(n);
    // 7 keys/candidate + 2 shared + 2 floor/target probes.
    let reads = n.saturating_mul(7).saturating_add(4);
    let undercount = reads.saturating_mul(64);
    assert!(
        w.proof_size() > undercount,
        "proof {} must exceed 64 B × {} reads ({})",
        w.proof_size(),
        reads,
        undercount
    );
    // Empty-ring pause path: TwapRing + two full spot quotes (9 reads). A
    // meter that only covers the old 2-read pause check must not start.
    run(|| {
        assert_ok!(open(1, MINT, COLLATERAL));
        force_deleverage(1);
        // Meter with no budget: cursor/pause reserve must abort before work.
        let mut meter = WeightMeter::with_limit(Weight::zero());
        assert_eq!(
            Lending::exhaust_deleverage_queue(&params(), &mut meter),
            0,
            "must reserve cursor + worst-case pause before any queue work"
        );
    });
}

/// Mid-block enable kills the floor (no uncharged live-set scan). Same-block
/// opens fail closed — every non-immune subnet is treated as in-zone until
/// the next metered `on_initialize` refresh.
#[test]
fn same_block_enable_and_open_tags_prune_target() {
    run(|| {
        set_ema(1.0);
        write(&key(b"test/ema", other()), U64F64::from_num(0.1));
        set_other_rank(0.1);
        assert!(!Lending::in_prune_zone(netuid(), &params()));
        assert!(Lending::in_prune_zone(other(), &params()));
        assert_ok!(Lending::set_enabled(RuntimeOrigin::root(), false));
        // Re-enable bumps generation and kills caches; floor waits for the
        // metered hook (constant-time extrinsic weight — no live_subnets()).
        assert_ok!(Lending::set_enabled(RuntimeOrigin::root(), true));
        assert!(
            PruneFloor::<Test>::get().is_none(),
            "enable must defer floor refresh to on_initialize"
        );
        assert!(
            PruneTarget::<Test>::get().is_none(),
            "enable must defer prune-target refresh to on_initialize"
        );
        assert!(
            Lending::in_prune_zone(other(), &params()),
            "missing floor fails closed"
        );
        assert!(
            Lending::in_prune_zone(netuid(), &params()),
            "missing floor fails closed for every non-immune subnet"
        );
        // Open while the floor is missing must be zone-tagged (fail closed).
        seed_twap_samples(other(), MIN_HEALTH_TWAP_SAMPLES, 0.1);
        NotionalEma::<Test>::insert(other(), U64F64::from_num(0.1));
        let owner = account(1);
        mint_tao(&owner, COLLATERAL + DEPOSIT);
        assert_ok!(Lending::open_short(
            RuntimeOrigin::signed(owner.clone()),
            other(),
            MINT,
            COLLATERAL,
            1
        ));
        let pos = Positions::<Test>::get(&owner, other()).expect("position");
        assert_eq!(
            pos.zone_minted, MINT,
            "same-block enable with missing floor must fail-closed tag"
        );
        assert_eq!(pos.zone_proceeds, pos.proceeds);
    });
}

/// Ranking stays dark while disabled, stays dark while the ring is warming,
/// and publishes once the ring holds a full window of real samples.
/// `set_enabled` no longer seeds from moving price — rings warm organically.
/// `on_subnet_registered` clears the ring so the new subnet starts dark.
#[test]
fn rank_stays_dark_until_full_window_of_real_samples() {
    run(|| {
        SynthParamsStore::<Test>::mutate(|p| {
            p.rank_days = 3;
            p.rank_day_blocks = 2;
            p.tao_ema_blocks = 4;
        });
        let update = || <Lending as LendingInterface<_>>::update_dereg_moving_price(netuid());
        let rank = || <Lending as LendingInterface<_>>::dereg_moving_price(netuid());
        RankRing::<Test>::remove(netuid());
        RankPrice::<Test>::remove(netuid());
        Enabled::<Test>::put(false);
        update();
        assert_eq!(rank(), None, "ranking must stay dark while disabled");
        assert_eq!(
            PoolTaoEma::<Test>::get(netuid()),
            Some(dereg::ema_step(None, POOL, 4)),
            "T_ema still tracks when disabled"
        );

        // Enable: ring stays empty, rank stays None until the ring warms.
        assert_ok!(Lending::set_enabled(RuntimeOrigin::root(), true));
        assert!(
            RankRing::<Test>::get(netuid())
                .map(|r| r.days.is_empty())
                .unwrap_or(true),
            "enable must NOT seed the ring"
        );
        assert_eq!(rank(), None, "ring empty after enable: still dark");

        // Run (rank_days * rank_day_blocks) = 6 blocks to warm the ring.
        // Blocks 1-2 → day 1; blocks 3-4 → day 2; blocks 5-6 → day 3 → full.
        for i in 1..=5 {
            update();
            assert_eq!(rank(), None, "ring partial at block {i}: still dark");
        }
        update(); // block 6: day 3 closes → ring full → SMA published
        assert!(rank().is_some(), "ring full: SMA published");

        // on_subnet_registered clears the ring and RankPrice for the new subnet.
        <Lending as LendingInterface<_>>::on_subnet_registered(netuid());
        assert!(
            RankRing::<Test>::get(netuid())
                .map(|r| r.days.is_empty())
                .unwrap_or(true),
            "on_subnet_registered must clear the ring"
        );
        assert_eq!(rank(), None, "new subnet: ring cleared, dark again");
        set_pool(POOL, POOL);
    });
}

#[test]
fn young_subnet_blocks_opens_until_health_twap_warms() {
    run(|| {
        TwapRing::<Test>::remove(netuid());
        assert_err!(open(1, MINT, COLLATERAL), Error::<Test>::YoungSubnet);
        seed_twap_samples(netuid(), MIN_HEALTH_TWAP_SAMPLES - 1, 1.0);
        assert_err!(open(1, MINT, COLLATERAL), Error::<Test>::YoungSubnet);
        seed_twap_samples(netuid(), MIN_HEALTH_TWAP_SAMPLES, 1.0);
        assert_ok!(open(1, MINT, COLLATERAL));
    });
}

#[test]
fn migration_does_not_seed_rank_ring_from_moving_price() {
    run(|| {
        set_ema(0.77);
        RankRing::<Test>::remove(netuid());
        RankPrice::<Test>::remove(netuid());
        Enabled::<Test>::put(false);
        crate::migrations::migrate_to_synth_shorts_v1::<Test>();
        assert!(!Enabled::<Test>::get());
        assert_eq!(
            <Lending as LendingInterface<_>>::dereg_moving_price(netuid()),
            None
        );
        assert!(
            RankRing::<Test>::get(netuid()).is_none(),
            "migration must not seed RankRing from capped moving price"
        );
    });
}

#[test]
fn dissolution_charges_accrued_interest_first() {
    run(|| {
        assert_ok!(open(1, MINT, COLLATERAL));
        let funds_before = position(1).funds();
        set_block(20);
        assert_ok!(Lending::start_dissolution(netuid()));
        let mut meter = WeightMeter::new();
        assert!(Lending::settle_positions_on_dissolution(
            netuid(),
            &mut meter
        ));
        write(b"test/dissolved", true);
        assert!(
            events()
                .iter()
                .any(|e| matches!(e, Event::InterestCharged { .. }))
        );
        assert!(!Positions::<Test>::contains_key(account(1), netuid()));
        assert!(tao(&account(1)) < funds_before + DEPOSIT);
    });
}

#[test]
fn protocol_buybacks_do_not_touch_net_mint_budget() {
    run(|| {
        assert_ok!(open(1, MINT, COLLATERAL));
        let net = netuid();
        assert_eq!(Lending::block_net_minted(net), i128::from(MINT));
        force_deleverage(1);
        assert!(Lending::deleverage_step(&account(1), net, &params()));
        assert_eq!(
            Lending::block_net_minted(net),
            i128::from(MINT),
            "deleverage must not debit BlockNetMinted"
        );
        set_residual(MINT / 4);
        Lending::retire_residual(net, MINT / 4, &params());
        assert_eq!(
            Lending::block_net_minted(net),
            i128::from(MINT),
            "residual retire must not debit BlockNetMinted"
        );
    });
}

#[test]
fn default_leftovers_respect_reserve_cap() {
    run(|| {
        assert_ok!(open(1, MINT, COLLATERAL));
        let cap = pool().0 * params().reserve_cap_bps as u64 / 10_000;
        let reserve = Lending::reserve_account(netuid());
        mint_tao(&reserve, cap);
        ResidualReserve::<Test>::insert(netuid(), cap);
        let owner = account(1);
        let pos = position(1);
        let dust = pos.funds();
        assert!(dust > 0);
        Lending::default_position(&owner, netuid(), &pos);
        assert!(ResidualReserve::<Test>::get(netuid()) <= cap);
        assert!(
            tao(&burn_account()) > 0,
            "overflow burns when reserve is full"
        );
    });
}

#[test]
fn uncollected_interest_is_tracked_when_collateral_dries() {
    run(|| {
        assert_ok!(open(1, MINT, COLLATERAL));
        SynthParamsStore::<Test>::mutate(|p| p.apr_bps = MAX_APR_BPS);
        set_block(100_000);
        Lending::on_idle(100_000, Weight::MAX);
        let shortfall = UncollectedInterest::<Test>::get(account(1), netuid());
        assert!(shortfall > 0, "shortfall must be recorded");
        assert!(
            events()
                .iter()
                .any(|e| matches!(e, Event::InterestUncollected { .. }))
        );
    });
}

// ---- solvency, issuance reconciliation, try_state coverage -----------------

/// I8: random books still satisfy pot ≥ payouts and `q × r` coverage bounds.
#[test]
fn random_state_solvency_property_i8() {
    use substrate_fixed::types::U64F64;

    new_test_ext().execute_with(|| {
        for who in 1u8..=4 {
            let mint = MINT + u64::from(who) * 37;
            let coll = COLLATERAL + u64::from(who) * 100;
            assert_ok!(open(who, mint, coll));
        }
        assert_ok!(close(2, MINT / 3));
        {
            for (_, _, pos) in Positions::<Test>::iter() {
                let rate = Lending::dissolution_rate(netuid());
                let owed = Lending::dissolution_owed(&pos, rate);
                assert!(
                    owed <= pos.funds(),
                    "I8: owed {owed} exceeds funds {}",
                    pos.funds()
                );
                if pos.zone_minted > 0 {
                    assert!(owed >= pos.zone_proceeds, "zone settles at proceeds");
                }
                let older = pos.minted.saturating_sub(pos.zone_minted);
                let qr = (U64F64::saturating_from_num(older).saturating_mul(rate))
                    .ceil()
                    .saturating_to_num::<u64>()
                    + pos.zone_proceeds;
                assert!(
                    qr <= pos.collateral.saturating_add(pos.proceeds),
                    "q×r ≤ C+P"
                );
            }
            let synthetic = SyntheticOutstanding::<Test>::get(netuid());
            let residual = ResidualAlpha::<Test>::get(netuid());
            assert_eq!(
                issuance(),
                base_issuance() + synthetic + residual,
                "I2: issuance reconciliation"
            );
            assert_ok!(Lending::start_dissolution(netuid()));
            let rate = Lending::dissolution_rate(netuid());
            let mut owed_sum = 0u64;
            for (_, _, pos) in Positions::<Test>::iter() {
                owed_sum = owed_sum.saturating_add(Lending::dissolution_owed(&pos, rate));
            }
            let reserve = ResidualReserve::<Test>::get(netuid());
            assert!(
                owed_sum.saturating_add(reserve)
                    <= Positions::<Test>::iter()
                        .map(|(_, _, p)| p.funds())
                        .sum::<u64>()
                        .saturating_add(reserve),
                "I8: pot can fund payouts from short funds"
            );
            Dissolutions::<Test>::remove(netuid());
        }
        assert_ok!(Lending::do_try_state());
    });
}

/// Adapter issuance deltas match synthetic / residual outstanding through
/// forced close, reserve retirement, and default.
#[test]
fn adapter_issuance_reconciliation_forced_close_retire_default() {
    new_test_ext().execute_with(|| {
        let track = || {
            (
                issuance(),
                SyntheticOutstanding::<Test>::get(netuid()),
                ResidualAlpha::<Test>::get(netuid()),
                burned_alpha(),
            )
        };
        let reconcile = || assert_eq!(issuance(), base_issuance() + track().1 + track().2);

        assert_ok!(open(1, MINT, COLLATERAL));
        reconcile();

        // User close: adapter issuance drops with burned alpha.
        set_block(11);
        let before = track();
        assert_ok!(close(1, MINT / 2));
        reconcile();
        assert!(track().1 < before.1);
        assert!(track().3 > before.3);

        // Forced deleverage close on a second short.
        assert_ok!(open(2, MINT, COLLATERAL));
        let (y, x) = pool();
        set_pool(y * 5, x);
        seed_twap_samples(netuid(), MIN_HEALTH_TWAP_SAMPLES, spot());
        force_deleverage(2);
        while Positions::<Test>::contains_key(account(2), netuid()) {
            step_block();
        }
        reconcile();
        assert!(ResidualAlpha::<Test>::get(netuid()) > 0);

        // Reserve retirement: adapter issuance tracks residual burn.
        let residual = ResidualAlpha::<Test>::get(netuid());
        ResidualReserve::<Test>::insert(netuid(), 50_000);
        mint_tao(&Lending::reserve_account(netuid()), 50_000);
        let burned_before = burned_alpha();
        Lending::retire_residual(netuid(), residual, &params());
        reconcile();
        assert!(burned_alpha() > burned_before);
        assert_ok!(Lending::do_try_state());
    });
}

/// Exercise try_state paths I2, I3, I5, I7, I8, I9 in one deterministic walk.
#[test]
fn try_state_covers_i2_i3_i5_i7_i8_i9_invariants() {
    run(|| {
        assert_ok!(open(1, MINT, COLLATERAL));
        // I7: BlockNetMinted cleared on finalize.
        assert!(Lending::block_net_minted(netuid()) != 0);
        Lending::on_finalize(System::block_number());
        assert_eq!(BlockNetMinted::<Test>::iter().count(), 0);
        check_invariants();

        // I5: interest charge capped at collateral.
        force_deleverage(1);
        let _ = Lending::deleverage_step(&account(1), netuid(), &params());
        check_invariants();

        let reserve = Lending::reserve_account(netuid());
        let cap = pool().0 * params().reserve_cap_bps as u64 / 10_000;
        mint_tao(&reserve, cap);
        ResidualReserve::<Test>::insert(netuid(), cap);
        set_residual(MINT / 3);
        check_invariants();

        // I8 / I9: dissolution pot funding and reserve cap.
        assert_ok!(Lending::start_dissolution(netuid()));
        loop {
            let mut m = WeightMeter::new();
            if Lending::settle_positions_on_dissolution(netuid(), &mut m) {
                break;
            }
        }
        write(b"test/dissolved", true);
        assert!(pot() > 0, "I8: pot receives short settlement");
        assert_ok!(Lending::do_try_state());
    });
}

// ---- new tests for HIGH 2 / HIGH 3 / MEDIUM 7 / MEDIUM 8 fixes ---------

/// A failed terminal_split_tao must not stall dissolution: position is removed,
/// loop advances, remaining positions settle normally.
#[test]
fn settle_failure_skips_position_and_advances() {
    run(|| {
        assert_ok!(open(1, MINT, COLLATERAL));
        assert_ok!(open(2, MINT, COLLATERAL));

        // Drain position 1's custody so terminal_split_tao fails (not enough TAO).
        let pos1 = position(1);
        set_tao(&pos1.custody, 0);

        assert_ok!(Lending::start_dissolution(netuid()));
        let mut meter = WeightMeter::new();
        assert!(Lending::settle_positions_on_dissolution(
            netuid(),
            &mut meter
        ));
        write(b"test/dissolved", true);

        // Both positions removed; SyntheticOutstanding zeroed.
        assert!(!Positions::<Test>::contains_key(account(1), netuid()));
        assert!(!Positions::<Test>::contains_key(account(2), netuid()));
        assert_eq!(SyntheticOutstanding::<Test>::get(netuid()), 0);

        let evts = events();
        assert!(
            evts.iter()
                .any(|e| matches!(e, Event::ShortSettleFailed { .. })),
            "ShortSettleFailed emitted for the drained position"
        );
        assert!(
            evts.iter()
                .any(|e| matches!(e, Event::ShortSettledOnDissolution { .. })),
            "ShortSettledOnDissolution emitted for the healthy position"
        );
        // Drained custody still records the book's stranded amount for sweep.
        assert!(
            StrandedCustody::<Test>::iter_prefix(netuid()).any(|(_, r)| r.due > 0 || r.refund > 0),
            "failed settle must record stranded custody"
        );
    });
}

#[test]
fn sweep_stranded_rejects_unrelated_accounts() {
    new_test_ext().execute_with(|| {
        let stranger = account(9);
        mint_tao(&stranger, 1_000_000);
        assert_noop!(
            Lending::sweep_stranded_funds(RuntimeOrigin::root(), netuid(), stranger),
            Error::<Test>::OwnerNotAllowed
        );
        let owner = account(1);
        let custody = Lending::position_account(&owner, netuid(), 10);
        let due = 20_000u64;
        let refund = 30_000u64;
        mint_tao(&custody, due + refund);
        StrandedCustody::<Test>::insert(
            netuid(),
            &custody,
            StrandedRecord {
                owner: owner.clone(),
                due,
                refund,
            },
        );
        let pot_before = pot();
        let owner_before = tao(&owner);
        assert_ok!(Lending::sweep_stranded_funds(
            RuntimeOrigin::root(),
            netuid(),
            custody.clone()
        ));
        assert!(StrandedCustody::<Test>::get(netuid(), &custody).is_none());
        assert_eq!(pot(), pot_before + due);
        assert_eq!(tao(&owner), owner_before + refund);
        assert_eq!(tao(&custody), 0);
    });
}

/// SyntheticOutstanding must never go below zero even when buy_back returns
/// more burned alpha than the position held (edge case in future callers).
/// We verify via close_short: open → partial close × 3 saturates at 0, not wraps.
#[test]
fn burn_from_position_caps_at_minted() {
    run(|| {
        assert_ok!(open(1, MINT, COLLATERAL));
        let outstanding_start = SyntheticOutstanding::<Test>::get(netuid());
        assert_eq!(outstanding_start, MINT);

        // Close all minted: SyntheticOutstanding → 0.
        assert_ok!(close(1, MINT));
        assert_eq!(
            SyntheticOutstanding::<Test>::get(netuid()),
            0,
            "SyntheticOutstanding cannot underflow"
        );
    });
}

/// I7 rejects stale BlockNetMinted tags; I8 rejects residual alpha with no
/// synthetic outstanding and no retire funding.
#[test]
fn try_state_rejects_stale_net_minted_and_unfunded_residual() {
    new_test_ext().execute_with(|| {
        assert_ok!(open(1, MINT, COLLATERAL));
        // Stale BlockNetMinted tag (I7).
        set_block(System::block_number() + 1);
        assert!(Lending::do_try_state().is_err(), "I7: stale BlockNetMinted");

        // Clear the tag, close the short, then leave residual with no reserve (I8).
        BlockNetMinted::<Test>::remove(netuid());
        assert_ok!(close(1, MINT));
        set_residual(MINT / 4);
        ResidualReserve::<Test>::remove(netuid());
        assert!(
            Lending::do_try_state().is_err(),
            "I8: residual without retire funding"
        );
    });
}

/// Empty rank ring warms block by block: None until full, Some after.
/// Disabling returns None; re-enabling clears rings so they rewarm.
#[test]
fn empty_rank_ring_warms_and_disable_returns_none() {
    run(|| {
        SynthParamsStore::<Test>::mutate(|p| {
            p.rank_days = 2;
            p.rank_day_blocks = 3;
        });
        let update = || <Lending as LendingInterface<_>>::update_dereg_moving_price(netuid());
        let rank = || <Lending as LendingInterface<_>>::dereg_moving_price(netuid());

        RankRing::<Test>::remove(netuid());
        RankPrice::<Test>::remove(netuid());

        // 2 days × 3 blocks = 6 updates to warm the ring.
        for i in 1..=5 {
            update();
            assert_eq!(rank(), None, "partial ring at update {i}: still dark");
        }
        update(); // 6th update closes day 2 → ring full → SMA published
        assert!(rank().is_some(), "ring full: SMA published");

        // Disable → rank returns None even though RankPrice is still stored.
        assert_ok!(Lending::set_enabled(RuntimeOrigin::root(), false));
        assert_eq!(
            rank(),
            None,
            "disabled: dark regardless of stored RankPrice"
        );

        // Re-enable bumps RankGeneration so stale rings read as dark.
        assert_ok!(Lending::set_enabled(RuntimeOrigin::root(), true));
        assert_eq!(
            rank(),
            None,
            "re-enable invalidates RankPrice by generation"
        );
        assert_ne!(
            RankRingGeneration::<Test>::get(netuid()),
            RankGeneration::<Test>::get(),
            "re-enable leaves ring generation behind the new epoch"
        );

        set_ema(0.0);
        assert_ok!(Lending::set_enabled(RuntimeOrigin::root(), true));
        assert_eq!(rank(), None, "zero-price ring: dark");
        set_ema(1.0);
    });
}

/// H1: an unwarm ring must not drag the prune-zone floor to zero via
/// `rank_price() == 0`. Floor uses RankPrice else moving price (same as
/// subtensor dereg selection), so a warm low subnet stays in the zone.
#[test]
fn prune_zone_ignores_unwarm_zero_rank_price() {
    run(|| {
        // Warm low subnet (netuid): RankPrice = 0.1.
        seed_rank_from_ema(netuid(), 0.1);
        set_ema(0.1);
        // Unwarm peer past immunity: no RankPrice, moving price = 1.0.
        // Without the fallback, `rank_price` would read as 0 and set floor=0.
        RankRing::<Test>::remove(other());
        RankPrice::<Test>::remove(other());
        RankRingGeneration::<Test>::remove(other());
        write(&key(b"test/ema", other()), U64F64::from_num(1.0));
        Lending::refresh_prune_floor_for_test();

        assert_eq!(
            Lending::rank_price(other()),
            U64F64::from_num(0),
            "unwarm ring still reports rank_price 0 for coverage"
        );
        assert_eq!(
            Lending::prune_rank_price(other()),
            U64F64::from_num(1.0),
            "prune floor falls back to moving price"
        );
        assert!(
            Lending::in_prune_zone(netuid(), &params()),
            "warm low subnet must stay in the prune zone"
        );
        assert!(!Lending::in_prune_zone(other(), &params()));
    });
}

/// H2 / MEDIUM 1: stalled queue entries (post-cutoff joiners) must not starve
/// forced closes — healthy cohort positions still progress at full pace in
/// the same block. Default open deposit remains 0.1 TAO.
#[test]
fn deleverage_skips_stalled_and_progresses_healthy_same_block() {
    run(|| {
        assert_eq!(
            SynthParams::default().deposit,
            100_000_000,
            "default open deposit is 0.1 TAO"
        );
        const _: () = assert!(MAX_DELEVERAGE_STEPS_PER_BLOCK <= 32);

        assert_ok!(open(1, MINT, COLLATERAL));
        assert_ok!(open(2, MINT, COLLATERAL));
        force_deleverage(1);
        force_deleverage(2);
        let now = System::block_number();
        let cutoff = NextDeleverageSeq::<Test>::get(netuid());
        let book = DeleveragingMinted::<Test>::get(netuid());
        // Wide pace so both cohort shares clear in one block; lock cutoff now.
        SynthParamsStore::<Test>::mutate(|p| p.pace_bps_per_block = 10_000);
        DeleverageBudget::<Test>::insert(netuid(), (now, u64::MAX / 4, book, cutoff));

        for who in 10u8..14 {
            assert_ok!(open(who, MINT, COLLATERAL));
            force_deleverage(who);
            let seq = DeleverageQueue::<Test>::get(netuid(), account(who)).expect("queued");
            assert!(seq > cutoff, "late joiner must be post-cutoff");
        }
        // Ensure at least one stalled key sorts before a healthy one so the
        // old break-on-first-stall bug would have blocked progress.
        let keys: Vec<_> = DeleverageQueue::<Test>::iter_keys()
            .filter(|(n, _)| *n == netuid())
            .collect();
        let first_stalled = keys.iter().find(
            |(_, o)| matches!(DeleverageQueue::<Test>::get(netuid(), o), Some(seq) if seq > cutoff),
        );
        let first_healthy = keys.iter().find(|(_, o)| {
            matches!(DeleverageQueue::<Test>::get(netuid(), o), Some(seq) if seq <= cutoff)
        });
        assert!(first_stalled.is_some() && first_healthy.is_some());

        let before_1 = position(1).minted;
        let before_2 = position(2).minted;
        let step_w = <Test as Config>::WeightInfo::deleverage_step();
        let mut meter = WeightMeter::with_limit(step_w.saturating_mul(64));
        let attempted = Lending::exhaust_deleverage_queue(&params(), &mut meter);
        assert!(
            attempted >= 6,
            "must visit stalled entries and both healthy ones; got {attempted}"
        );
        let progressed = |who: u8, before: u64| {
            match Positions::<Test>::get(account(who), netuid()) {
                Some(p) => p.minted < before,
                None => true, // fully closed / defaulted counts as progress
            }
        };
        assert!(
            progressed(1, before_1) && progressed(2, before_2),
            "healthy positions must progress at full pace in the same block"
        );
        let stepped: Vec<_> = events()
            .iter()
            .filter_map(|e| match e {
                Event::DeleverageStep { who, .. } => Some(who.clone()),
                _ => None,
            })
            .collect();
        assert!(
            stepped.contains(&account(1)) && stepped.contains(&account(2)),
            "both healthy cohort positions must step in this block"
        );
    });
}

#[test]
fn failed_default_writes_stranded_custody_for_owner_refund() {
    new_test_ext().execute_with(|| {
        assert_ok!(open(1, MINT, COLLATERAL));
        let pos = position(1);
        write(b"test/fail_burn", true);
        Lending::default_position(&account(1), netuid(), &pos);
        write(b"test/fail_burn", false);
        let stranded = StrandedCustody::<Test>::get(netuid(), &pos.custody)
            .expect("failed default must record stranded custody");
        assert_eq!(stranded.owner, account(1));
        // Reserve collect still succeeded; only the deposit burn failed, so
        // pot-due is 0 and the owner is not refunded the deposit.
        assert_eq!(stranded.due, 0);
        assert_eq!(
            stranded.refund, 0,
            "default must not refund deposit or overflow to the owner"
        );
        assert_eq!(
            tao(&pos.custody),
            pos.deposit,
            "failed deposit burn leaves deposit on custody for the sweep burn path"
        );
        assert!(!Positions::<Test>::contains_key(account(1), netuid()));
    });
}

/// M2: a paused subnet with ≥32 queued keys must not consume the full step
/// budget — healthy positions on a later subnet still progress in the same
/// block after a cheap pause skip.
#[test]
fn deleverage_skips_paused_subnet_and_progresses_healthy_same_block() {
    // Synthetic paused-queue keys have no positions; skip try-state.
    new_test_ext().execute_with(|| {
        const PAUSED: u16 = 1;
        let paused: NetUid = PAUSED.into();
        // Low TWAP median with spot ≈ 1.0 ⇒ buybacks_paused.
        seed_twap_samples(paused, MIN_HEALTH_TWAP_SAMPLES, 0.1);
        assert!(Lending::buybacks_paused(paused, &params()));

        for i in 0u8..40 {
            DeleverageQueue::<Test>::insert(paused, account(i), u64::from(i));
        }
        assert_ok!(open(1, MINT, COLLATERAL));
        assert_ok!(open(2, MINT, COLLATERAL));
        force_deleverage(1);
        force_deleverage(2);
        let now = System::block_number();
        let cutoff = NextDeleverageSeq::<Test>::get(netuid());
        let book = DeleveragingMinted::<Test>::get(netuid());
        SynthParamsStore::<Test>::mutate(|p| p.pace_bps_per_block = 10_000);
        DeleverageBudget::<Test>::insert(netuid(), (now, u64::MAX / 4, book, cutoff));

        // Paused netuid 1 sorts ahead of healthy netuid 64.
        let first = DeleverageQueue::<Test>::iter_keys().next().expect("queue");
        assert_eq!(first.0, paused);

        let before_1 = position(1).minted;
        let before_2 = position(2).minted;
        let step_w = <Test as Config>::WeightInfo::deleverage_step();
        let mut meter = WeightMeter::with_limit(step_w.saturating_mul(64));
        let attempted = Lending::exhaust_deleverage_queue(&params(), &mut meter);
        assert!(
            attempted >= 2,
            "healthy cohort must still be attempted; got {attempted}"
        );
        assert!(
            attempted < 40,
            "paused flood must not consume one full step each; got {attempted}"
        );
        let progressed = |who: u8, before: u64| match Positions::<Test>::get(account(who), netuid())
        {
            Some(p) => p.minted < before,
            None => true,
        };
        assert!(
            progressed(1, before_1) && progressed(2, before_2),
            "healthy positions must progress after the paused-subnet skip"
        );
    });
}

#[test]
fn notional_ema_clears_on_register_and_seeds_from_first_price() {
    run(|| {
        NotionalEma::<Test>::insert(netuid(), U64F64::from_num(9.0));
        Lending::on_subnet_registered(netuid());
        assert_eq!(NotionalEma::<Test>::get(netuid()), None);
        set_pool(POOL * 2, POOL);
        let spot = MockPool::current_alpha_price(netuid());
        Lending::update_dereg_moving_price(netuid());
        assert_eq!(
            NotionalEma::<Test>::get(netuid()),
            Some(spot),
            "first tick seeds NotionalEma from spot"
        );
        set_pool(POOL * 3, POOL);
        Lending::update_dereg_moving_price(netuid());
        let after = NotionalEma::<Test>::get(netuid()).expect("ema");
        assert!(after > spot, "later ticks move the EMA");
        assert_ne!(after, MockPool::current_alpha_price(netuid()));
    });
}

/// Failed interest burn must not debit custody proceeds / uncollected interest.
#[test]
fn drain_uncollected_interest_skips_debit_when_burn_fails() {
    new_test_ext().execute_with(|| {
        assert_ok!(open(1, MINT, COLLATERAL));
        let mut pos = position(1);
        UncollectedInterest::<Test>::insert(account(1), netuid(), 1_000);
        // Empty custody so burn_interest_tao fails transfer. Skip try_state
        // (I3) because the custody is intentionally underfunded.
        set_tao(&pos.custody, 0);
        pos.proceeds = 5_000;
        pos.collateral = 0;
        let proceeds_before = pos.proceeds;
        Lending::drain_uncollected_interest_for_test(&account(1), netuid(), &mut pos);
        assert_eq!(pos.proceeds, proceeds_before, "no debit on burn failure");
        assert_eq!(
            UncollectedInterest::<Test>::get(account(1), netuid()),
            1_000,
            "owed interest stays recorded"
        );
    });
}
