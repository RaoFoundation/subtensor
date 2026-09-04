#![allow(
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::expect_used
)]

pub(crate) mod mock;

use frame_support::{assert_err, assert_ok};
use sp_core::U256;
use sp_runtime::{Perbill, Percent};
use subtensor_runtime_common::{NetUid, TaoBalance};
use subtensor_swap_interface::Perquintill;

use crate::{Closer, Error, Event, Footprint, PalletHotkey, Params, Side, position::*};
use mock::*;

const POOL_TAO: u64 = 1_000 * TAO;
const POOL_ALPHA: u64 = 4_000 * TAO;
const DEPOSIT: u64 = 10 * TAO;

fn netuid() -> NetUid {
    NetUid::from(1u16)
}

fn alice() -> U256 {
    U256::from(1)
}
fn bob() -> U256 {
    U256::from(2)
}
fn alice_hotkey() -> U256 {
    U256::from(101)
}

fn setup() {
    add_dynamic_network(netuid(), POOL_TAO, POOL_ALPHA);
    add_balance(&alice(), 100 * TAO);
    add_balance(&bob(), 100 * TAO);
    let _ = SubtensorModule::create_account_if_non_existent(&alice(), &alice_hotkey());
}

fn open(who: U256, side: Side, cushion: u64) -> sp_runtime::DispatchResult {
    Derivatives::open(RuntimeOrigin::signed(who), netuid(), side, cushion.into())
}

fn close(caller: U256, owner: U256, side: Side) -> sp_runtime::DispatchResult {
    Derivatives::close(RuntimeOrigin::signed(caller), owner, netuid(), side)
}

fn one_day_fee(fee_per_day: TaoBalance) -> u64 {
    accrued_fee(fee_per_day, 1).into()
}

fn assert_close(a: u64, b: u64, tolerance: u64) {
    let diff = a.abs_diff(b);
    assert!(
        diff <= tolerance,
        "{a} vs {b}: differ by {diff} > {tolerance}"
    );
}

/// `(proceeds, debt, escrow)` as raw units, whichever side the position is.
fn legs(pos: &Position<u64>) -> (u64, u64, u64) {
    match pos.legs {
        Legs::Short {
            proceeds,
            debt,
            escrow,
        } => (proceeds.into(), debt.into(), escrow.into()),
        Legs::Long {
            proceeds,
            debt,
            escrow,
        } => (proceeds.into(), debt.into(), escrow.into()),
    }
}

/// `(tao_to_owner, fee_paid, shortfall)` of the latest `PositionClosed`.
fn last_closed_event() -> (u64, u64, u64) {
    System::events()
        .into_iter()
        .rev()
        .find_map(|record| match record.event {
            RuntimeEvent::Derivatives(Event::PositionClosed {
                tao_to_owner,
                fee_paid,
                shortfall,
                ..
            }) => Some((
                tao_to_owner.into(),
                fee_paid.into(),
                match shortfall {
                    Lent::Alpha(amount) => amount.into(),
                    Lent::Tao(amount) => amount.into(),
                },
            )),
            _ => None,
        })
        .expect("PositionClosed event")
}

// ── Pallet hotkey ────────────────────────────────────────────────────────────

#[test]
fn upgrade_claims_a_fresh_hotkey_for_the_pallet_account() {
    new_test_ext().execute_with(|| {
        let hotkey = pallet_hotkey();
        assert_eq!(Some(hotkey), Derivatives::hotkey_candidate(0));
        assert!(SubtensorModule::coldkey_owns_hotkey(
            &pallet_account(),
            &hotkey
        ));

        // A second upgrade keeps the one already claimed.
        <Derivatives as frame_support::traits::OnRuntimeUpgrade>::on_runtime_upgrade();
        assert_eq!(pallet_hotkey(), hotkey);
    });
}

#[test]
fn claim_skips_a_hotkey_someone_registered_first() {
    new_test_ext().execute_with(|| {
        // A later upgrade block: different parent hash, different candidates.
        System::set_parent_hash(sp_core::H256::repeat_byte(7));
        PalletHotkey::<Test>::kill();
        let taken = Derivatives::hotkey_candidate(0).unwrap();
        let _ = SubtensorModule::create_account_if_non_existent(&alice(), &taken);

        Derivatives::claim_hotkey();

        let hotkey = pallet_hotkey();
        assert_eq!(Some(hotkey), Derivatives::hotkey_candidate(1));
        assert!(SubtensorModule::coldkey_owns_hotkey(
            &pallet_account(),
            &hotkey
        ));
        assert!(SubtensorModule::coldkey_owns_hotkey(&alice(), &taken));
    });
}

#[test]
fn nothing_opens_until_the_hotkey_is_claimed() {
    new_test_ext().execute_with(|| {
        setup();
        PalletHotkey::<Test>::kill();
        assert_err!(
            open(alice(), Side::Short, DEPOSIT),
            Error::<Test>::PalletHotkeyUnset
        );
        assert_eq!(balance(&alice()), 100 * TAO);
    });
}

// ── Open ─────────────────────────────────────────────────────────────────────

#[test]
fn open_short_with_tao_lifts_and_sells() {
    new_test_ext().execute_with(|| {
        setup();
        let (t0, a0) = reserves(netuid());
        let out0 = alpha_out(netuid());
        let stake0 = total_stake();
        let flow0 = tao_flow(netuid());

        assert_ok!(open(alice(), Side::Short, DEPOSIT));

        let pos = position(&alice(), netuid(), Side::Short).unwrap();
        let (proceeds, debt, escrow) = legs(&pos);
        assert!(matches!(pos.legs, Legs::Short { .. }));
        // 1x leverage: phi = 10 / 1000 = 1%.
        assert_eq!(escrow, POOL_TAO / 100);
        assert_eq!(debt, POOL_ALPHA / 100);
        assert_eq!(u64::from(pos.exposure_tao), POOL_TAO / 100);
        // Selling 1% of alpha into a pool that just lost 1% pays a bit under 1% of TAO.
        assert!(proceeds > 0 && proceeds < POOL_TAO / 100);
        assert_eq!(pos.expires_at, 1 + 216_000);

        // Alice paid the deposit; the pallet holds deposit + escrow + proceeds.
        assert_eq!(balance(&alice()), 100 * TAO - DEPOSIT);
        assert_eq!(balance(&pallet_account()), DEPOSIT + escrow + proceeds);
        // The lifted alpha went back into the pool when sold: AlphaIn is whole, AlphaOut too.
        let (t1, a1) = reserves(netuid());
        assert_eq!(a1, a0);
        assert_eq!(alpha_out(netuid()), out0);
        assert_eq!(t1, t0 - escrow - proceeds);
        assert_eq!(total_stake(), stake0 - escrow - proceeds);
        assert_eq!(tao_flow(netuid()), flow0);
        assert_eq!(
            Footprint::<Test>::get(netuid(), Side::Short),
            escrow + proceeds
        );
    });
}

#[test]
fn open_long_with_tao_lifts_and_buys() {
    new_test_ext().execute_with(|| {
        setup();
        let (t0, a0) = reserves(netuid());
        let out0 = alpha_out(netuid());
        let stake0 = total_stake();

        assert_ok!(open(alice(), Side::Long, DEPOSIT));

        let pos = position(&alice(), netuid(), Side::Long).unwrap();
        let (proceeds, debt, escrow) = legs(&pos);
        assert!(matches!(pos.legs, Legs::Long { .. }));
        // Longs run at 2x: a 10 TAO cushion lifts 2% of the 1000 TAO pool.
        assert_eq!(debt, POOL_TAO / 50);
        assert_eq!(escrow, POOL_ALPHA / 50);
        assert!(proceeds > 0 && proceeds < POOL_ALPHA / 50);

        // Pallet holds the deposit in TAO and escrow + proceeds as stake.
        assert_eq!(balance(&pallet_account()), DEPOSIT);
        assert_eq!(
            stake(&pallet_account(), &pallet_hotkey(), netuid()),
            escrow + proceeds
        );
        // TAO went out and came straight back in; alpha left the pool.
        let (t1, a1) = reserves(netuid());
        assert_eq!(t1, t0);
        assert_eq!(total_stake(), stake0);
        assert_eq!(a1, a0 - escrow - proceeds);
        assert_eq!(alpha_out(netuid()), out0 + escrow + proceeds);
    });
}

#[test]
fn open_rejects_bad_inputs() {
    new_test_ext().execute_with(|| {
        setup();
        assert_err!(
            open(alice(), Side::Short, TAO / 100),
            Error::<Test>::DepositTooLow
        );
        assert_err!(open(alice(), Side::Short, 0), Error::<Test>::ZeroExposure);
        assert_err!(
            Derivatives::open(
                RuntimeOrigin::signed(alice()),
                NetUid::from(9u16),
                Side::Short,
                DEPOSIT.into()
            ),
            Error::<Test>::SubnetNotDynamic
        );

        let mut params = Params::<Test>::get();
        params.longs_enabled = false;
        assert_ok!(Derivatives::sudo_set_params(RuntimeOrigin::root(), params));
        assert_err!(
            open(alice(), Side::Long, DEPOSIT),
            Error::<Test>::SideDisabled
        );

        assert_ok!(open(alice(), Side::Short, DEPOSIT));
        assert_err!(
            open(alice(), Side::Short, DEPOSIT),
            Error::<Test>::PositionExists
        );
    });
}

#[test]
fn footprint_cap_rejects_stacking() {
    new_test_ext().execute_with(|| {
        setup();
        // kappa = 10% of the TAO reserve. Each 1x/10 TAO short takes ~2% (phi * (2 - phi)).
        let mut params = Params::<Test>::get();
        params.max_pool_share = Percent::from_percent(5);
        assert_ok!(Derivatives::sudo_set_params(RuntimeOrigin::root(), params));

        assert_ok!(open(alice(), Side::Short, DEPOSIT));
        assert_ok!(open(bob(), Side::Short, DEPOSIT));
        let charlie = U256::from(3);
        add_balance(&charlie, 100 * TAO);
        // Third one would push the footprint over 5%.
        assert_err!(
            open(charlie, Side::Short, DEPOSIT),
            Error::<Test>::PoolCapExceeded
        );
        // A single oversized position is rejected outright.
        assert_err!(
            open(charlie, Side::Long, 60 * TAO),
            Error::<Test>::PoolCapExceeded
        );
    });
}

#[test]
fn fee_per_day_is_priced_by_side_and_frozen_at_open() {
    new_test_ext().execute_with(|| {
        setup();
        let params = Params::<Test>::get();
        assert_ok!(open(alice(), Side::Short, DEPOSIT));
        // The short's lift shrank the TAO reserve, so the long's phi is a little over 2%.
        let long_phi = pool_fraction(200, DEPOSIT, reserves(netuid()).0).unwrap();
        assert_ok!(open(bob(), Side::Long, DEPOSIT));

        // The short lifts phi = 1% at 1x, the long about 2% at 2x. The short pays C * phi in
        // TAO, whatever its exposure; the long pays r * exposure. Both are scaled by the size
        // factor for their own phi.
        let short = position(&alice(), netuid(), Side::Short).unwrap();
        let long = position(&bob(), netuid(), Side::Long).unwrap();
        assert_eq!(
            u64::from(short.fee_per_day),
            size_factor(
                Perquintill::from_percent(1),
                u64::from(params.short_fee_per_day) / 100
            )
        );
        assert_eq!(
            u64::from(long.fee_per_day),
            size_factor(
                long_phi,
                params
                    .long_rate_per_day
                    .mul_floor(u64::from(long.exposure_tao))
            )
        );

        // Changing the parameters after the open does not reprice a running position.
        let mut changed = params.clone();
        changed.short_fee_per_day = (u64::from(params.short_fee_per_day) * 10).into();
        changed.long_rate_per_day = Perbill::from_percent(50);
        assert_ok!(Derivatives::sudo_set_params(RuntimeOrigin::root(), changed));
        assert_eq!(
            position(&alice(), netuid(), Side::Short)
                .unwrap()
                .fee_per_day,
            short.fee_per_day
        );
        assert_ok!(close(alice(), alice(), Side::Short));
        let (_, fee_paid, _) = last_closed_event();
        assert_eq!(fee_paid, one_day_fee(short.fee_per_day));
    });
}

// ── Close ────────────────────────────────────────────────────────────────────

#[test]
fn short_open_close_same_block_returns_deposit_minus_one_day_fee() {
    new_test_ext().execute_with(|| {
        setup();
        let (t0, a0) = reserves(netuid());
        let out0 = alpha_out(netuid());
        let stake0 = total_stake();
        let w0 = balancer_weight(netuid());
        let p0 = price(netuid());

        assert_ok!(open(alice(), Side::Short, DEPOSIT));
        let pos = position(&alice(), netuid(), Side::Short).unwrap();
        assert_ok!(close(alice(), alice(), Side::Short));

        let fee = one_day_fee(pos.fee_per_day);
        let (tao_back, fee_paid, shortfall) = last_closed_event();
        assert_eq!(fee_paid, fee);
        assert_eq!(shortfall, 0);
        // Buying back Q alpha costs almost exactly what selling it paid: the round trip loses
        // only rounding, so Alice gets her deposit back minus the fee.
        assert_close(tao_back, DEPOSIT - fee, 100);
        assert_eq!(balance(&alice()), 100 * TAO - DEPOSIT + tao_back);
        assert!(position(&alice(), netuid(), Side::Short).is_none());
        assert_eq!(Footprint::<Test>::get(netuid(), Side::Short), 0);
        assert_eq!(balance(&pallet_account()), 0);

        // Pool got everything back plus the fee. Price is where it started; the fee is added
        // one-sided, so the weights move by at most its share of the reserve.
        let (t1, a1) = reserves(netuid());
        assert_close(t1, t0 + fee, 100);
        assert_close(a1, a0, 100);
        assert_close(alpha_out(netuid()), out0, 100);
        assert_close(total_stake(), stake0 + fee, 100);
        assert_close(
            w0.deconstruct(),
            balancer_weight(netuid()).deconstruct(),
            sp_runtime::Perquintill::from_rational(fee, t0).deconstruct(),
        );
        // Price is preserved to about one part in a billion; the rest is fixed-point rounding.
        assert_close(
            p0.to_bits() as u64,
            price(netuid()).to_bits() as u64,
            (p0.to_bits() as u64) >> 30,
        );
    });
}

#[test]
fn long_open_close_same_block_returns_deposit_minus_one_day_fee() {
    new_test_ext().execute_with(|| {
        setup();
        let (t0, a0) = reserves(netuid());
        let out0 = alpha_out(netuid());
        let stake0 = total_stake();

        assert_ok!(open(alice(), Side::Long, DEPOSIT));
        let pos = position(&alice(), netuid(), Side::Long).unwrap();
        assert_ok!(close(alice(), alice(), Side::Long));

        let fee = one_day_fee(pos.fee_per_day);
        let (tao_back, fee_paid, shortfall) = last_closed_event();
        assert_eq!(fee_paid, fee);
        assert_eq!(shortfall, 0);
        assert_close(tao_back, DEPOSIT - fee, 100);
        assert_eq!(balance(&pallet_account()), 0);
        assert_eq!(stake(&pallet_account(), &pallet_hotkey(), netuid()), 0);

        let (t1, a1) = reserves(netuid());
        assert_close(t1, t0 + fee, 100);
        assert_close(a1, a0, 100);
        assert_close(alpha_out(netuid()), out0, 100);
        assert_close(total_stake(), stake0 + fee, 100);
    });
}

#[test]
fn short_profits_when_price_falls_and_loses_when_it_rises() {
    new_test_ext().execute_with(|| {
        setup();
        assert_ok!(open(alice(), Side::Short, DEPOSIT));
        // Bob dumps alpha: price falls.
        give_stake(&bob(), &alice_hotkey(), netuid(), 400 * TAO);
        assert_ok!(SubtensorModule::remove_stake(
            RuntimeOrigin::signed(bob()),
            alice_hotkey(),
            netuid(),
            (400 * TAO).into()
        ));
        assert_ok!(close(alice(), alice(), Side::Short));
        let (tao_back, _, shortfall) = last_closed_event();
        assert_eq!(shortfall, 0);
        assert!(tao_back > DEPOSIT, "short should profit: {tao_back}");
    });

    new_test_ext().execute_with(|| {
        setup();
        assert_ok!(open(alice(), Side::Short, DEPOSIT));
        // Bob buys alpha: price rises.
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(bob()),
            alice_hotkey(),
            netuid(),
            (50 * TAO).into()
        ));
        assert_ok!(close(alice(), alice(), Side::Short));
        let (tao_back, _, shortfall) = last_closed_event();
        assert_eq!(shortfall, 0);
        assert!(tao_back < DEPOSIT, "short should lose: {tao_back}");
    });
}

#[test]
fn underwater_short_settles_with_shortfall_and_pool_is_never_short() {
    new_test_ext().execute_with(|| {
        setup();
        let (t0, _) = reserves(netuid());
        assert_ok!(open(alice(), Side::Short, DEPOSIT));
        // Price triples: alpha is now far more expensive than N + P can buy.
        let whale = U256::from(7);
        add_balance(&whale, 5_000 * TAO);
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(whale),
            alice_hotkey(),
            netuid(),
            (2_000 * TAO).into()
        ));
        assert_ok!(close(alice(), alice(), Side::Short));
        let (tao_back, fee_paid, shortfall) = last_closed_event();
        assert_eq!(tao_back, 0);
        assert_eq!(fee_paid, 0);
        assert!(shortfall > 0);
        // Everything the pallet held for the position went back to the pool.
        assert_eq!(balance(&pallet_account()), 0);
        assert!(reserves(netuid()).0 > t0);
        assert!(position(&alice(), netuid(), Side::Short).is_none());
    });
}

#[test]
fn long_at_two_x_survives_a_moderate_drop_and_the_pool_is_whole() {
    new_test_ext().execute_with(|| {
        setup();
        assert_ok!(open(alice(), Side::Long, DEPOSIT));
        // Price falls about 30%: the proceeds alpha sells for less than D, the cushion covers
        // the gap, the pool gets D back and Alice keeps the rest.
        give_stake(&bob(), &alice_hotkey(), netuid(), 800 * TAO);
        assert_ok!(SubtensorModule::remove_stake(
            RuntimeOrigin::signed(bob()),
            alice_hotkey(),
            netuid(),
            (800 * TAO).into()
        ));
        assert_ok!(close(alice(), alice(), Side::Long));
        let (tao_back, fee_paid, shortfall) = last_closed_event();
        assert_eq!(shortfall, 0);
        assert!(fee_paid > 0);
        assert!(tao_back > 0 && tao_back < DEPOSIT, "tao_back = {tao_back}");
        assert_eq!(balance(&pallet_account()), 0);
        assert_eq!(stake(&pallet_account(), &pallet_hotkey(), netuid()), 0);
    });
}

#[test]
fn long_at_two_x_is_underwater_once_the_price_halves() {
    new_test_ext().execute_with(|| {
        setup();
        assert_ok!(open(alice(), Side::Long, DEPOSIT));
        // At 2x the cushion is D / 2, so a collapse past a halving leaves a shortfall. Alice
        // gets nothing, the pool takes everything the pallet still holds.
        give_stake(&bob(), &alice_hotkey(), netuid(), 20_000 * TAO);
        assert_ok!(SubtensorModule::remove_stake(
            RuntimeOrigin::signed(bob()),
            alice_hotkey(),
            netuid(),
            (20_000 * TAO).into()
        ));
        assert_ok!(close(alice(), alice(), Side::Long));
        let (tao_back, fee_paid, shortfall) = last_closed_event();
        assert_eq!(tao_back, 0);
        assert_eq!(fee_paid, 0);
        assert!(shortfall > 0);
        assert_eq!(balance(&pallet_account()), 0);
        assert_eq!(stake(&pallet_account(), &pallet_hotkey(), netuid()), 0);
        assert!(position(&alice(), netuid(), Side::Long).is_none());
    });
}

// ── Per-subnet overrides ──────────────────────────────────────────────────────

#[test]
fn subnet_override_pauses_one_side_and_replaces_the_cap() {
    new_test_ext().execute_with(|| {
        setup();
        assert_ok!(open(alice(), Side::Short, DEPOSIT));

        let paused = SubnetOverride {
            shorts_enabled: false,
            longs_enabled: true,
            max_pool_share: Some(Percent::from_percent(1)),
        };
        assert_ok!(Derivatives::sudo_set_subnet_override(
            RuntimeOrigin::root(),
            netuid(),
            Some(paused)
        ));
        System::assert_last_event(
            Event::SubnetOverrideSet {
                netuid: netuid(),
                override_: Some(paused),
            }
            .into(),
        );

        // Shorts are paused here but not elsewhere: the global switch is still on.
        assert!(Params::<Test>::get().shorts_enabled);
        assert_err!(
            open(bob(), Side::Short, DEPOSIT),
            Error::<Test>::SideDisabled
        );
        // A paused side can still close, and roll cannot reopen.
        assert_err!(
            Derivatives::roll(
                RuntimeOrigin::signed(alice()),
                netuid(),
                Side::Short,
                0.into()
            ),
            Error::<Test>::SideDisabled
        );
        assert!(position(&alice(), netuid(), Side::Short).is_some());
        assert_ok!(close(alice(), alice(), Side::Short));

        // Longs are open but capped at 1% here instead of the global 10%: a 2% lift fails,
        // a 0.5% lift passes.
        assert_err!(
            open(bob(), Side::Long, DEPOSIT),
            Error::<Test>::PoolCapExceeded
        );
        assert_ok!(open(bob(), Side::Long, DEPOSIT / 4));

        // Removing the override puts the subnet back on the global parameters.
        assert_ok!(Derivatives::sudo_set_subnet_override(
            RuntimeOrigin::root(),
            netuid(),
            None
        ));
        assert!(crate::SubnetOverrides::<Test>::get(netuid()).is_none());
        assert_ok!(open(alice(), Side::Short, DEPOSIT));
    });
}

#[test]
fn subnet_override_requires_root_and_a_non_zero_cap() {
    new_test_ext().execute_with(|| {
        setup();
        let override_ = SubnetOverride {
            shorts_enabled: true,
            longs_enabled: true,
            max_pool_share: Some(Percent::zero()),
        };
        assert_err!(
            Derivatives::sudo_set_subnet_override(
                RuntimeOrigin::signed(alice()),
                netuid(),
                Some(override_)
            ),
            sp_runtime::DispatchError::BadOrigin
        );
        assert_err!(
            Derivatives::sudo_set_subnet_override(RuntimeOrigin::root(), netuid(), Some(override_)),
            Error::<Test>::InvalidParams
        );
        assert!(crate::SubnetOverrides::<Test>::get(netuid()).is_none());
    });
}

// ── Roll ─────────────────────────────────────────────────────────────────────

#[test]
fn roll_settles_and_reopens_with_the_payout() {
    new_test_ext().execute_with(|| {
        setup();
        assert_ok!(open(alice(), Side::Short, DEPOSIT));
        let before = position(&alice(), netuid(), Side::Short).unwrap();
        let balance_before = balance(&alice());
        System::set_block_number(100);

        assert_ok!(Derivatives::roll(
            RuntimeOrigin::signed(alice()),
            netuid(),
            Side::Short,
            0.into()
        ));

        let after = position(&alice(), netuid(), Side::Short).unwrap();
        let (tao_back, ..) = last_closed_event();
        assert_eq!(after.opened_at, 100);
        assert_eq!(after.cushion.tao(), TaoBalance::from(tao_back));
        assert!(tao_back < DEPOSIT, "one day of fee came off the cushion");
        assert!(after.opened_at > before.opened_at);
        assert_eq!(balance(&alice()), balance_before, "nothing left the pallet");
        assert_eq!(
            Footprint::<Test>::get(netuid(), Side::Short),
            after.legs.footprint()
        );
    });
}

#[test]
fn failed_roll_leaves_the_old_position_untouched() {
    new_test_ext().execute_with(|| {
        setup();
        assert_ok!(open(alice(), Side::Short, DEPOSIT));
        let before = position(&alice(), netuid(), Side::Short).unwrap();
        let footprint = Footprint::<Test>::get(netuid(), Side::Short);
        let balance_before = balance(&alice());
        let pool_before = reserves(netuid());

        // A top-up that pushes the new position past the pool cap: the reopen fails after the
        // settle ran, and both roll back.
        assert_err!(
            Derivatives::roll(
                RuntimeOrigin::signed(alice()),
                netuid(),
                Side::Short,
                (55 * TAO).into()
            ),
            Error::<Test>::PoolCapExceeded
        );

        assert_eq!(position(&alice(), netuid(), Side::Short), Some(before));
        assert_eq!(Footprint::<Test>::get(netuid(), Side::Short), footprint);
        assert_eq!(balance(&alice()), balance_before);
        assert_eq!(reserves(netuid()), pool_before);
    });
}

// ── Expiry ───────────────────────────────────────────────────────────────────

#[test]
fn only_owner_may_close_before_expiry() {
    new_test_ext().execute_with(|| {
        setup();
        assert_ok!(open(alice(), Side::Short, DEPOSIT));
        assert_err!(
            close(bob(), alice(), Side::Short),
            Error::<Test>::NotExpired
        );
        assert_err!(close(bob(), bob(), Side::Short), Error::<Test>::NoPosition);

        let expires_at = position(&alice(), netuid(), Side::Short)
            .unwrap()
            .expires_at;
        System::set_block_number(expires_at);
        assert_ok!(close(bob(), alice(), Side::Short));
        assert!(position(&alice(), netuid(), Side::Short).is_none());
    });
}

#[test]
fn on_idle_sweeps_expired_positions_and_spills_full_blocks() {
    new_test_ext().execute_with(|| {
        setup();
        let charlie = U256::from(3);
        add_balance(&charlie, 100 * TAO);
        // MaxExpiriesPerBlock = 2, so the third position lands one block later.
        assert_ok!(open(alice(), Side::Short, DEPOSIT));
        assert_ok!(open(bob(), Side::Short, DEPOSIT));
        assert_ok!(open(charlie, Side::Short, DEPOSIT));
        let a = position(&alice(), netuid(), Side::Short)
            .unwrap()
            .expires_at;
        let c = position(&charlie, netuid(), Side::Short)
            .unwrap()
            .expires_at;
        assert_eq!(c, a + 1);

        // Nothing happens before expiry.
        System::set_block_number(a - 1);
        run_idle();
        assert!(position(&alice(), netuid(), Side::Short).is_some());

        System::set_block_number(a);
        run_idle();
        assert!(position(&alice(), netuid(), Side::Short).is_none());
        assert!(position(&bob(), netuid(), Side::Short).is_none());
        assert!(position(&charlie, netuid(), Side::Short).is_some());

        System::set_block_number(c);
        run_idle();
        assert!(position(&charlie, netuid(), Side::Short).is_none());
        assert_eq!(Footprint::<Test>::get(netuid(), Side::Short), 0);
        assert!(crate::Expiring::<Test>::get(a).is_empty());
        assert!(crate::Expiring::<Test>::get(c).is_empty());
    });
}

#[test]
fn early_close_removes_expiry_entry() {
    new_test_ext().execute_with(|| {
        setup();
        assert_ok!(open(alice(), Side::Long, DEPOSIT));
        let at = position(&alice(), netuid(), Side::Long).unwrap().expires_at;
        assert_eq!(crate::Expiring::<Test>::get(at).len(), 1);
        assert_ok!(close(alice(), alice(), Side::Long));
        assert!(crate::Expiring::<Test>::get(at).is_empty());
    });
}

// ── Dissolution ──────────────────────────────────────────────────────────────

#[test]
fn dissolution_unwinds_all_four_kinds_at_par() {
    new_test_ext().execute_with(|| {
        setup();
        let (t0, a0) = reserves(netuid());
        let out0 = alpha_out(netuid());

        assert_ok!(open(alice(), Side::Short, DEPOSIT));
        assert_ok!(open(bob(), Side::Long, DEPOSIT));

        // Dissolve: the subnet is no longer "added" but its account and pool still exist.
        assert_ok!(SubtensorModule::do_dissolve_network(netuid()));
        settle_all_for_dissolution(netuid());

        for (who, side) in [(alice(), Side::Short), (bob(), Side::Long)] {
            assert!(position(&who, netuid(), side).is_none());
        }
        // Cushions back, no fee.
        assert_eq!(balance(&alice()), 100 * TAO);
        assert_eq!(balance(&bob()), 100 * TAO);
        // Pool is whole and the pallet holds nothing.
        let (t1, a1) = reserves(netuid());
        assert_close(t1, t0, 100);
        assert_close(a1, a0, 100);
        assert_close(alpha_out(netuid()), out0, 100);
        assert_eq!(balance(&pallet_account()), 0);
        assert_eq!(stake(&pallet_account(), &pallet_hotkey(), netuid()), 0);
        assert_eq!(Footprint::<Test>::get(netuid(), Side::Short), 0);
        assert_eq!(Footprint::<Test>::get(netuid(), Side::Long), 0);
        assert!(
            crate::OpenByNetuid::<Test>::iter_prefix(netuid())
                .next()
                .is_none()
        );
        let closers: Vec<_> = System::events()
            .into_iter()
            .filter_map(|r| match r.event {
                RuntimeEvent::Derivatives(Event::PositionClosed { closed_by, .. }) => {
                    Some(closed_by)
                }
                _ => None,
            })
            .collect();
        assert_eq!(closers.len(), 2);
        assert!(closers.iter().all(|c| *c == Closer::Dissolution));
    });
}

#[test]
fn dissolution_hook_is_bounded_by_weight() {
    new_test_ext().execute_with(|| {
        setup();
        assert_ok!(open(alice(), Side::Short, DEPOSIT));
        assert_ok!(open(bob(), Side::Long, DEPOSIT));
        assert_ok!(SubtensorModule::do_dissolve_network(netuid()));

        let mut meter = frame_support::weights::WeightMeter::with_limit(
            <() as crate::weights::WeightInfo>::close(),
        );
        assert!(
            !<Derivatives as subtensor_runtime_common::SubnetDissolveHook>::on_subnet_dissolve(
                netuid(),
                &mut meter
            )
        );
        assert_eq!(
            crate::OpenByNetuid::<Test>::iter_prefix(netuid()).count(),
            1
        );
        settle_all_for_dissolution(netuid());
        assert_eq!(
            crate::OpenByNetuid::<Test>::iter_prefix(netuid()).count(),
            0
        );
    });
}

#[test]
fn positions_are_isolated_per_side_and_conserve_alpha() {
    new_test_ext().execute_with(|| {
        setup();
        let supply0 = reserves(netuid()).1 + alpha_out(netuid());
        assert_ok!(open(alice(), Side::Short, DEPOSIT));
        assert_ok!(open(alice(), Side::Long, DEPOSIT));
        assert_eq!(reserves(netuid()).1 + alpha_out(netuid()), supply0);
        assert_ok!(close(alice(), alice(), Side::Short));
        assert_ok!(close(alice(), alice(), Side::Long));
        assert_close(reserves(netuid()).1 + alpha_out(netuid()), supply0, 100);
        assert_eq!(tao_flow(netuid()), 0);
    });
}

#[test]
fn set_params_requires_root() {
    new_test_ext().execute_with(|| {
        let params = Params::<Test>::get();
        assert_err!(
            Derivatives::sudo_set_params(RuntimeOrigin::signed(alice()), params.clone()),
            sp_runtime::DispatchError::BadOrigin
        );
        assert_ok!(Derivatives::sudo_set_params(RuntimeOrigin::root(), params));
    });
}
