#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]

use frame_support::{assert_noop, assert_ok};
use sp_core::H256;
use sp_runtime::DispatchError;
use subtensor_runtime_common::{
    TusdBalance,
    rails::{OriginChain, StableAsset},
};

use crate::{
    Balances, Bridge, BridgeOutstanding, BridgeRoute, Bridges, Error, Event, TotalIssuance, mock::*,
};

const BASE: u32 = 8453;
const ETHEREUM: u32 = 1;
const ALICE: u64 = 1;
const BOB: u64 = 2;
/// One tUSD.
const USD: u64 = 1_000_000_000;

fn tusd(amount: u64) -> TusdBalance {
    TusdBalance::new(amount)
}

fn route(domain: u32) -> BridgeRoute {
    BridgeRoute {
        origin: OriginChain::Evm(u64::from(domain)),
        domain,
        portal: H256::repeat_byte(domain as u8),
        asset: StableAsset::Usdc,
    }
}

/// Registers and enables a bridge with a `cap` and a window of `limit` per 300 blocks.
fn open_bridge(id: u32, cap: u64, limit: u64) {
    assert_ok!(Tusd::set_bridge(
        RuntimeOrigin::root(),
        id,
        route(id),
        tusd(cap),
        tusd(limit),
        300
    ));
    assert_ok!(Tusd::set_bridge_enabled(RuntimeOrigin::root(), id, true));
}

fn bridge(id: u32) -> Bridge {
    Bridges::<Test>::get(id).unwrap()
}

fn assert_invariants() {
    Tusd::do_try_state().unwrap();
    let held: u64 = Balances::<Test>::iter().map(|(_, b)| u64::from(b)).sum();
    assert_eq!(held, u64::from(TotalIssuance::<Test>::get()));
    assert!(
        Balances::<Test>::iter().all(|(_, b)| u64::from(b) >= MIN_BALANCE),
        "an account holds dust"
    );
}

#[test]
fn new_bridge_starts_disabled_and_cannot_mint() {
    new_test_ext().execute_with(|| {
        assert_ok!(Tusd::set_bridge(
            RuntimeOrigin::root(),
            BASE,
            route(BASE),
            tusd(1_000 * USD),
            tusd(1_000 * USD),
            300
        ));
        assert!(!bridge(BASE).enabled);
        assert_eq!(Tusd::bridge_of(BASE, route(BASE).portal), Some(BASE));
        assert_noop!(
            Tusd::mint(BASE, &ALICE, tusd(USD)),
            Error::<Test>::BridgeDisabled
        );
        assert_noop!(
            Tusd::mint(ETHEREUM, &ALICE, tusd(USD)),
            Error::<Test>::BridgeUnknown
        );
    });
}

#[test]
fn only_admin_registers_bridges() {
    new_test_ext().execute_with(|| {
        for origin in [
            RuntimeOrigin::signed(ALICE),
            RuntimeOrigin::signed(GUARDIAN),
        ] {
            assert_noop!(
                Tusd::set_bridge(origin, BASE, route(BASE), tusd(USD), tusd(USD), 300),
                DispatchError::BadOrigin
            );
        }
    });
}

#[test]
fn mint_credits_the_account_the_bridge_and_the_issuance() {
    new_test_ext().execute_with(|| {
        open_bridge(BASE, 1_000 * USD, 1_000 * USD);
        assert_ok!(Tusd::mint(BASE, &ALICE, tusd(250 * USD)));

        assert_eq!(Tusd::balance(&ALICE), tusd(250 * USD));
        assert_eq!(BridgeOutstanding::<Test>::get(BASE), tusd(250 * USD));
        assert_eq!(TotalIssuance::<Test>::get(), tusd(250 * USD));
        System::assert_last_event(
            Event::Minted {
                bridge: BASE,
                to: ALICE,
                amount: tusd(250 * USD),
            }
            .into(),
        );
        assert_invariants();
    });
}

#[test]
fn mint_stops_at_the_bridge_cap() {
    new_test_ext().execute_with(|| {
        open_bridge(BASE, 100 * USD, u64::MAX);
        assert_ok!(Tusd::mint(BASE, &ALICE, tusd(100 * USD)));
        assert_noop!(
            Tusd::mint(BASE, &ALICE, tusd(MIN_BALANCE)),
            Error::<Test>::MintCapExceeded
        );
        // A burn makes room again.
        assert_ok!(Tusd::burn(BASE, &ALICE, tusd(40 * USD)));
        assert_ok!(Tusd::mint(BASE, &BOB, tusd(40 * USD)));
        assert_noop!(
            Tusd::mint(BASE, &BOB, tusd(1)),
            Error::<Test>::MintCapExceeded
        );
        assert_invariants();
    });
}

#[test]
fn mint_is_rate_limited_and_the_window_refills() {
    new_test_ext().execute_with(|| {
        open_bridge(BASE, u64::MAX, 300 * USD);
        assert_ok!(Tusd::mint(BASE, &ALICE, tusd(300 * USD)));
        assert_noop!(
            Tusd::mint(BASE, &ALICE, tusd(USD)),
            Error::<Test>::MintRateLimited
        );

        // 1 tUSD refills per block: 10 blocks later exactly 10 fit.
        System::set_block_number(11);
        assert_noop!(
            Tusd::mint(BASE, &ALICE, tusd(10 * USD + 1)),
            Error::<Test>::MintRateLimited
        );
        assert_ok!(Tusd::mint(BASE, &ALICE, tusd(10 * USD)));

        // Burning does not refill the window: only time does.
        assert_ok!(Tusd::burn(BASE, &ALICE, tusd(200 * USD)));
        assert_noop!(
            Tusd::mint(BASE, &ALICE, tusd(USD)),
            Error::<Test>::MintRateLimited
        );
        assert_invariants();
    });
}

#[test]
fn each_bridge_is_bounded_on_its_own() {
    new_test_ext().execute_with(|| {
        open_bridge(BASE, 100 * USD, u64::MAX);
        open_bridge(ETHEREUM, 50 * USD, u64::MAX);
        assert_ok!(Tusd::mint(BASE, &ALICE, tusd(100 * USD)));
        assert_ok!(Tusd::mint(ETHEREUM, &ALICE, tusd(50 * USD)));
        assert_eq!(TotalIssuance::<Test>::get(), tusd(150 * USD));

        // tUSD is fungible: any holder redeems through any bridge, but each bridge only up
        // to what it brought in.
        assert_noop!(
            Tusd::burn(ETHEREUM, &ALICE, tusd(50 * USD + 1)),
            Error::<Test>::RedeemExceedsOutstanding
        );
        assert_ok!(Tusd::burn(ETHEREUM, &ALICE, tusd(50 * USD)));
        assert_eq!(BridgeOutstanding::<Test>::get(ETHEREUM), tusd(0));
        assert_eq!(BridgeOutstanding::<Test>::get(BASE), tusd(100 * USD));
        assert_eq!(Tusd::balance(&ALICE), tusd(100 * USD));
        assert_invariants();
    });
}

#[test]
fn burn_needs_the_balance() {
    new_test_ext().execute_with(|| {
        open_bridge(BASE, 100 * USD, u64::MAX);
        assert_ok!(Tusd::mint(BASE, &ALICE, tusd(100 * USD)));
        assert_noop!(
            Tusd::burn(BASE, &BOB, tusd(USD)),
            Error::<Test>::InsufficientBalance
        );
        assert_noop!(
            Tusd::burn(ETHEREUM, &ALICE, tusd(USD)),
            Error::<Test>::BridgeUnknown
        );
        assert_ok!(Tusd::burn(BASE, &ALICE, tusd(100 * USD)));
        assert!(!Balances::<Test>::contains_key(ALICE));
        assert_eq!(TotalIssuance::<Test>::get(), tusd(0));
        assert_invariants();
    });
}

#[test]
fn disabled_bridge_neither_mints_nor_burns() {
    new_test_ext().execute_with(|| {
        open_bridge(BASE, 100 * USD, u64::MAX);
        assert_ok!(Tusd::mint(BASE, &ALICE, tusd(10 * USD)));
        assert_ok!(Tusd::set_bridge_enabled(RuntimeOrigin::root(), BASE, false));
        assert_noop!(
            Tusd::mint(BASE, &ALICE, tusd(USD)),
            Error::<Test>::BridgeDisabled
        );
        assert_noop!(
            Tusd::burn(BASE, &ALICE, tusd(USD)),
            Error::<Test>::BridgeDisabled
        );
        // Holders still transfer.
        assert_ok!(Tusd::transfer(RuntimeOrigin::signed(ALICE), BOB, tusd(USD)));
    });
}

#[test]
fn guardian_disables_but_never_enables() {
    new_test_ext().execute_with(|| {
        open_bridge(BASE, 100 * USD, u64::MAX);
        assert_noop!(
            Tusd::set_bridge_enabled(RuntimeOrigin::signed(ALICE), BASE, false),
            DispatchError::BadOrigin
        );
        assert_ok!(Tusd::set_bridge_enabled(
            RuntimeOrigin::signed(GUARDIAN),
            BASE,
            false
        ));
        assert!(!bridge(BASE).enabled);
        assert_noop!(
            Tusd::set_bridge_enabled(RuntimeOrigin::signed(GUARDIAN), BASE, true),
            DispatchError::BadOrigin
        );
        assert_ok!(Tusd::set_bridge_enabled(RuntimeOrigin::root(), BASE, true));
        assert!(bridge(BASE).enabled);
        assert_noop!(
            Tusd::set_bridge_enabled(RuntimeOrigin::root(), ETHEREUM, false),
            Error::<Test>::BridgeUnknown
        );
    });
}

#[test]
fn changing_a_bridges_limits_keeps_its_accounting() {
    new_test_ext().execute_with(|| {
        open_bridge(BASE, 1_000 * USD, 300 * USD);
        assert_ok!(Tusd::mint(BASE, &ALICE, tusd(300 * USD)));

        // A larger cap and a smaller window.
        assert_ok!(Tusd::set_bridge(
            RuntimeOrigin::root(),
            BASE,
            route(BASE),
            tusd(2_000 * USD),
            tusd(150 * USD),
            300
        ));

        let changed = bridge(BASE);
        assert!(changed.enabled, "a change must not disable the bridge");
        assert_eq!(changed.mint_cap, tusd(2_000 * USD));
        assert_eq!(BridgeOutstanding::<Test>::get(BASE), tusd(300 * USD));
        assert_eq!(
            changed.mint_window.used,
            300 * USD,
            "consumption survives the change"
        );
        assert_noop!(
            Tusd::mint(BASE, &ALICE, tusd(USD)),
            Error::<Test>::MintRateLimited
        );
        assert_invariants();
    });
}

#[test]
fn the_route_of_a_bridge_is_set_once() {
    new_test_ext().execute_with(|| {
        open_bridge(BASE, 1_000 * USD, 300 * USD);
        assert_ok!(Tusd::mint(BASE, &ALICE, tusd(300 * USD)));

        // What the bridge has outstanding is in the vault of its portal. No other portal,
        // domain, chain or stablecoin can stand for it.
        for moved in [
            BridgeRoute {
                portal: H256::repeat_byte(0xAB),
                ..route(BASE)
            },
            BridgeRoute {
                domain: 7,
                ..route(BASE)
            },
            BridgeRoute {
                origin: OriginChain::Evm(7),
                ..route(BASE)
            },
            BridgeRoute {
                asset: StableAsset::Usdt,
                ..route(BASE)
            },
        ] {
            assert_noop!(
                Tusd::set_bridge(
                    RuntimeOrigin::root(),
                    BASE,
                    moved,
                    tusd(1_000 * USD),
                    tusd(300 * USD),
                    300
                ),
                Error::<Test>::RouteIsFixed
            );
        }
        // Not when it has nothing outstanding either, and not when it is disabled.
        assert_ok!(Tusd::burn(BASE, &ALICE, tusd(300 * USD)));
        assert_ok!(Tusd::set_bridge_enabled(RuntimeOrigin::root(), BASE, false));
        assert_noop!(
            Tusd::set_bridge(
                RuntimeOrigin::root(),
                BASE,
                BridgeRoute {
                    portal: H256::repeat_byte(0xAB),
                    ..route(BASE)
                },
                tusd(1_000 * USD),
                tusd(300 * USD),
                300
            ),
            Error::<Test>::RouteIsFixed
        );
        assert_eq!(Tusd::bridge_of(BASE, route(BASE).portal), Some(BASE));
        assert_eq!(bridge(BASE).route, route(BASE));
        assert_invariants();
    });
}

#[test]
fn a_portal_belongs_to_one_bridge() {
    new_test_ext().execute_with(|| {
        open_bridge(BASE, 100 * USD, u64::MAX);
        assert_noop!(
            Tusd::set_bridge(
                RuntimeOrigin::root(),
                ETHEREUM,
                route(BASE),
                tusd(USD),
                tusd(USD),
                300
            ),
            Error::<Test>::PortalAlreadyRegistered
        );
        // Re-setting the same bridge with its own portal is fine.
        assert_ok!(Tusd::set_bridge(
            RuntimeOrigin::root(),
            BASE,
            route(BASE),
            tusd(USD),
            tusd(USD),
            300
        ));
    });
}

#[test]
fn cap_below_outstanding_winds_a_bridge_down() {
    new_test_ext().execute_with(|| {
        open_bridge(BASE, 100 * USD, u64::MAX);
        assert_ok!(Tusd::mint(BASE, &ALICE, tusd(100 * USD)));
        assert_ok!(Tusd::set_bridge(
            RuntimeOrigin::root(),
            BASE,
            route(BASE),
            tusd(0),
            tusd(u64::MAX),
            300
        ));
        assert_noop!(
            Tusd::mint(BASE, &ALICE, tusd(USD)),
            Error::<Test>::MintCapExceeded
        );
        assert_ok!(Tusd::burn(BASE, &ALICE, tusd(100 * USD)));
        assert_invariants();
    });
}

#[test]
fn transfer_moves_tusd() {
    new_test_ext().execute_with(|| {
        open_bridge(BASE, 100 * USD, u64::MAX);
        assert_ok!(Tusd::mint(BASE, &ALICE, tusd(100 * USD)));
        assert_ok!(Tusd::transfer(
            RuntimeOrigin::signed(ALICE),
            BOB,
            tusd(30 * USD)
        ));
        assert_eq!(Tusd::balance(&ALICE), tusd(70 * USD));
        assert_eq!(Tusd::balance(&BOB), tusd(30 * USD));
        System::assert_last_event(
            Event::Transferred {
                from: ALICE,
                to: BOB,
                amount: tusd(30 * USD),
            }
            .into(),
        );
        assert_noop!(
            Tusd::transfer(RuntimeOrigin::signed(BOB), ALICE, tusd(30 * USD + 1)),
            Error::<Test>::InsufficientBalance
        );
        assert_noop!(
            Tusd::transfer(RuntimeOrigin::none(), ALICE, tusd(USD)),
            DispatchError::BadOrigin
        );
        // The whole balance can leave; the empty entry is removed.
        assert_ok!(Tusd::transfer(
            RuntimeOrigin::signed(BOB),
            ALICE,
            tusd(30 * USD)
        ));
        assert!(!Balances::<Test>::contains_key(BOB));
        assert_eq!(TotalIssuance::<Test>::get(), tusd(100 * USD));
        assert_invariants();
    });
}

#[test]
fn transfer_never_leaves_dust() {
    new_test_ext().execute_with(|| {
        open_bridge(BASE, 100 * USD, u64::MAX);
        assert_ok!(Tusd::mint(BASE, &ALICE, tusd(USD)));
        // Dust at the receiver.
        assert_noop!(
            Tusd::transfer(RuntimeOrigin::signed(ALICE), BOB, tusd(MIN_BALANCE - 1)),
            Error::<Test>::BelowMinimumBalance
        );
        // Dust at the sender.
        assert_noop!(
            Tusd::transfer(RuntimeOrigin::signed(ALICE), BOB, tusd(USD - 1)),
            Error::<Test>::BelowMinimumBalance
        );
        // A mint cannot create dust either, and a failed mint changes nothing.
        assert_noop!(
            Tusd::mint(BASE, &BOB, tusd(MIN_BALANCE - 1)),
            Error::<Test>::BelowMinimumBalance
        );
        assert_ok!(Tusd::transfer(
            RuntimeOrigin::signed(ALICE),
            BOB,
            tusd(MIN_BALANCE)
        ));
        assert_invariants();
    });
}

#[test]
fn transfer_to_self_and_of_nothing_change_nothing() {
    new_test_ext().execute_with(|| {
        open_bridge(BASE, 100 * USD, u64::MAX);
        assert_ok!(Tusd::mint(BASE, &ALICE, tusd(USD)));
        assert_ok!(Tusd::transfer(
            RuntimeOrigin::signed(ALICE),
            ALICE,
            tusd(USD)
        ));
        assert_ok!(Tusd::transfer(RuntimeOrigin::signed(ALICE), BOB, tusd(0)));
        assert_noop!(
            Tusd::transfer(RuntimeOrigin::signed(ALICE), ALICE, tusd(USD + 1)),
            Error::<Test>::InsufficientBalance
        );
        assert_eq!(Tusd::balance(&ALICE), tusd(USD));
        assert!(!Balances::<Test>::contains_key(BOB));
        assert_invariants();
    });
}

#[test]
fn mint_and_issuance_do_not_overflow() {
    new_test_ext().execute_with(|| {
        open_bridge(BASE, u64::MAX, u64::MAX);
        open_bridge(ETHEREUM, u64::MAX, u64::MAX);
        assert_ok!(Tusd::mint(BASE, &ALICE, tusd(u64::MAX)));
        assert_noop!(
            Tusd::mint(ETHEREUM, &BOB, tusd(USD)),
            sp_runtime::ArithmeticError::Overflow
        );
        assert_invariants();
    });
}

/// Random mints, burns and transfers across two bridges and four accounts never break the
/// ledger invariants, whether each step succeeds or not.
#[test]
fn ledger_invariants_hold_under_random_operations() {
    new_test_ext().execute_with(|| {
        open_bridge(BASE, 5_000 * USD, 2_000 * USD);
        open_bridge(ETHEREUM, 3_000 * USD, 1_000 * USD);
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        let mut next = move |bound: u64| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state % bound
        };
        let mut succeeded = 0;
        for step in 0..4_000u64 {
            System::set_block_number(1 + step / 4);
            let bridge = if next(2) == 0 { BASE } else { ETHEREUM };
            let who = 1 + next(4);
            let amount = tusd(next(400 * USD));
            let result = match next(3) {
                0 => Tusd::mint(bridge, &who, amount),
                1 => Tusd::burn(bridge, &who, amount),
                _ => Tusd::transfer(RuntimeOrigin::signed(who), 1 + next(4), amount),
            };
            succeeded += u64::from(result.is_ok());
            assert_invariants();
            for id in [BASE, ETHEREUM] {
                assert!(
                    BridgeOutstanding::<Test>::get(id)
                        <= Bridges::<Test>::get(id).unwrap().mint_cap
                );
            }
        }
        assert!(succeeded > 500, "the walk must exercise successful paths");
    });
}
