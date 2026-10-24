#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used)]

use frame_support::traits::ExtendedDispatchable;
use frame_support::{BoundedVec, assert_noop, assert_ok};
use sp_core::U256;
use sp_runtime::traits::Dispatchable;
use sp_std::collections::btree_map::BTreeMap;
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::{AlphaBalance, GammaBalance, NetUid, ProxyType, TaoBalance};
use subtensor_swap_interface::SwapHandler;

use super::mock::*;
use crate::*;

const TAO: u64 = 1_000_000_000;
const GAMMA: u64 = 1_000_000_000;
/// 400 dollars per TAO.
const PRICE: u64 = 400 * GAMMA;
const ORACLE: u64 = 9_000;
const OWNER_HOTKEY: u64 = 1;
const OWNER_COLDKEY: u64 = 2;
const COLDKEY: u64 = 3;
const HOTKEY: u64 = 4;

/// Gives `netuid` a pool of `tao` TAO and `alpha` alpha, enables gamma network-wide and on
/// it, feeds `PRICE`, steps a block and settles its gamma price at spot.
fn enable_gamma(netuid: NetUid, tao: u64, alpha: u64) {
    setup_reserves(netuid, tao.into(), alpha.into());
    GammaEnabled::<Test>::put(true);
    SubnetGammaEnabled::<Test>::insert(netuid, true);
    TaoUsdPriceOracle::<Test>::put(U256::from(ORACLE));
    assert_ok!(SubtensorModule::feed_tao_usd_price(
        RuntimeOrigin::signed(U256::from(ORACLE)),
        PRICE
    ));
    step_block(1);
    let spot = <Test as Config>::SwapInterface::current_alpha_price(netuid);
    SubnetGammaPrice::<Test>::insert(netuid, spot);
}

fn gamma_subnet() -> NetUid {
    let netuid = add_dynamic_network(&U256::from(OWNER_HOTKEY), &U256::from(OWNER_COLDKEY));
    enable_gamma(netuid, 1_000 * TAO, 1_000 * TAO);
    netuid
}

/// The index lists a hotkey for a coldkey exactly when it holds a deposit of that coldkey.
fn assert_gamma_invariants(netuid: NetUid) {
    let deposits: Vec<_> = GammaDeposits::<Test>::iter_prefix(netuid).collect();
    for (hotkey, deposit) in &deposits {
        assert!(ColdkeyGammaHotkeys::<Test>::get(netuid, deposit.coldkey).contains(hotkey));
    }
    let listed: usize = ColdkeyGammaHotkeys::<Test>::iter_prefix(netuid)
        .map(|(coldkey, hotkeys)| {
            for hotkey in &hotkeys {
                assert_eq!(
                    GammaDeposits::<Test>::get(netuid, hotkey).map(|deposit| deposit.coldkey),
                    Some(coldkey)
                );
            }
            hotkeys.len()
        })
        .sum();
    assert_eq!(listed, deposits.len());
}

fn mint(netuid: NetUid, coldkey: u64, hotkey: u64, tao: u64) -> GammaBalance {
    add_balance_to_coldkey_account(&U256::from(coldkey), (tao + TAO).into());
    let before = GammaDeposits::<Test>::get(netuid, U256::from(hotkey))
        .map_or(GammaBalance::ZERO, |deposit| deposit.balance);
    assert_ok!(SubtensorModule::mint_gamma(
        RuntimeOrigin::signed(U256::from(coldkey)),
        U256::from(hotkey),
        netuid,
        tao.into(),
        GammaBalance::ZERO,
    ));
    GammaDeposits::<Test>::get(netuid, U256::from(hotkey))
        .unwrap()
        .balance
        - before
}

fn spends(entries: &[(u64, u64)]) -> BoundedVec<(U256, GammaBalance), ConstU32<MAX_GAMMA_SPENDS>> {
    BoundedVec::truncate_from(
        entries
            .iter()
            .map(|(hotkey, amount)| (U256::from(*hotkey), GammaBalance::from(*amount)))
            .collect(),
    )
}

fn deposit(netuid: NetUid, hotkey: u64) -> GammaDeposit<U256> {
    GammaDeposits::<Test>::get(netuid, U256::from(hotkey)).unwrap()
}

fn gamma_events() -> Vec<Event<Test>> {
    System::events()
        .into_iter()
        .filter_map(|record| match record.event {
            RuntimeEvent::SubtensorModule(
                event @ (Event::GammaMinted { .. }
                | Event::GammaMoved { .. }
                | Event::GammaDepositClosed { .. }
                | Event::GammaDepositSpenderSet { .. }
                | Event::GammaDepositDailySpendCapSet { .. }
                | Event::GammaSpent { .. }),
            ) => Some(event),
            _ => None,
        })
        .collect()
}

/// The alpha, value and gamma of the last `GammaMinted` event.
fn minted() -> (AlphaBalance, TaoBalance, GammaBalance) {
    gamma_events()
        .into_iter()
        .rev()
        .find_map(|event| match event {
            Event::GammaMinted {
                alpha,
                value,
                gamma,
                ..
            } => Some((alpha, value, gamma)),
            _ => None,
        })
        .unwrap()
}

// =============================================================================
// TAO/USD price
// =============================================================================

#[test]
fn tao_usd_price_first_feed_is_accepted() {
    new_test_ext(7).execute_with(|| {
        TaoUsdPriceOracle::<Test>::put(U256::from(ORACLE));
        assert_ok!(SubtensorModule::feed_tao_usd_price(
            RuntimeOrigin::signed(U256::from(ORACLE)),
            PRICE
        ));
        assert_eq!(
            TaoUsdPrice::<Test>::get(),
            Some(TaoUsdPriceState {
                accepted: PRICE,
                feed: PRICE,
                fed_at: 7,
            })
        );
        System::assert_has_event(Event::TaoUsdPriceFed { price: PRICE }.into());
    });
}

#[test]
fn tao_usd_price_rises_by_the_step_and_falls_to_the_feed() {
    new_test_ext(1).execute_with(|| {
        let at = |accepted: u64, feed: u64, blocks: u64| {
            SubtensorModule::tao_usd_price_at(
                &TaoUsdPriceState {
                    accepted,
                    feed,
                    fed_at: 100,
                },
                100 + blocks,
            )
        };
        let (low, high) = (400 * GAMMA, 500 * GAMMA);
        let step = TAO_USD_PRICE_MAX_STEP.mul_ceil(low);

        assert_eq!(at(low, high, 0), low);
        assert_eq!(at(low, high, 1), low + step);
        assert_eq!(at(low, high, 100), low + 100 * step);
        assert_eq!(at(low, high, 10_000), high);
        assert_eq!(at(low, high, u64::MAX - 100), high);
        assert_eq!(at(high, low, 0), low);
        // Any price rises, however low.
        assert_eq!(at(1, high, 1), 2);
    });
}

#[test]
fn mint_tao_usd_price_requires_a_fresh_feed() {
    new_test_ext(1).execute_with(|| {
        let put = |accepted: u64, feed: u64| {
            TaoUsdPrice::<Test>::put(TaoUsdPriceState {
                accepted,
                feed,
                fed_at: 100,
            });
        };
        let unavailable = Err(Error::<Test>::TaoUsdPriceUnavailable);
        assert_eq!(SubtensorModule::mint_tao_usd_price(100), unavailable);

        put(PRICE, PRICE);
        assert_eq!(SubtensorModule::mint_tao_usd_price(150), Ok(PRICE));
        assert_eq!(SubtensorModule::mint_tao_usd_price(151), unavailable);

        put(300 * GAMMA, 400 * GAMMA);
        assert_eq!(SubtensorModule::mint_tao_usd_price(100), Ok(300 * GAMMA));
    });
}

#[test]
fn feed_is_refused_for_other_signers_and_zero() {
    new_test_ext(1).execute_with(|| {
        let oracle = RuntimeOrigin::signed(U256::from(ORACLE));
        assert_noop!(
            SubtensorModule::feed_tao_usd_price(oracle.clone(), PRICE),
            DispatchError::BadOrigin
        );
        TaoUsdPriceOracle::<Test>::put(U256::from(ORACLE));
        assert_noop!(
            SubtensorModule::feed_tao_usd_price(RuntimeOrigin::signed(U256::from(COLDKEY)), PRICE),
            DispatchError::BadOrigin
        );
        assert_noop!(
            SubtensorModule::feed_tao_usd_price(oracle.clone(), 0),
            Error::<Test>::InvalidValue
        );
        assert_ok!(SubtensorModule::feed_tao_usd_price(oracle.clone(), PRICE));

        // A lower feed is accepted at once; a higher one is risen toward.
        System::set_block_number(2);
        assert_ok!(SubtensorModule::feed_tao_usd_price(
            oracle.clone(),
            PRICE / 2
        ));
        assert_ok!(SubtensorModule::feed_tao_usd_price(oracle, PRICE));
        assert_eq!(TaoUsdPrice::<Test>::get().unwrap().accepted, PRICE / 2);
        assert_eq!(
            SubtensorModule::mint_tao_usd_price(3),
            Ok(PRICE / 2 + TAO_USD_PRICE_MAX_STEP.mul_ceil(PRICE / 2))
        );
    });
}

// =============================================================================
// Enablement and gamma price
// =============================================================================

#[test]
fn gamma_price_falls_to_spot_and_rises_at_the_fast_rate() {
    new_test_ext(1).execute_with(|| {
        let netuid = gamma_subnet();
        let median = SubtensorModule::get_median_subnet_alpha_price();
        let disabled = add_dynamic_network(&U256::from(10), &U256::from(11));
        let far = add_dynamic_network(&U256::from(12), &U256::from(13));
        let unset = add_dynamic_network(&U256::from(14), &U256::from(15));
        let spot = |netuid| <Test as Config>::SwapInterface::current_alpha_price(netuid);
        let price = SubnetGammaPrice::<Test>::get;
        let alpha = U64F64::from_num(core::f64::consts::LN_2)
            / U64F64::from_num(BASKET_FAST_EMA_HALF_LIFE_BLOCKS);

        // Registration sets it to the median subnet alpha price.
        assert_eq!(price(disabled), median);

        // Down to spot at once, up from below by the fast moving price's weight of the gap, at
        // most of itself, and from unset to spot, gamma or not.
        SubnetGammaPrice::<Test>::insert(netuid, spot(netuid) * U64F64::from_num(2));
        let half = spot(disabled) / U64F64::from_num(2);
        SubnetGammaPrice::<Test>::insert(disabled, half);
        let tenth = spot(far) / U64F64::from_num(10);
        SubnetGammaPrice::<Test>::insert(far, tenth);
        SubnetGammaPrice::<Test>::remove(unset);
        step_block(1);
        assert_eq!(price(netuid), spot(netuid));
        assert_eq!(price(disabled), half + (spot(disabled) - half) * alpha);
        assert_eq!(price(far), tenth + tenth * alpha);
        assert_eq!(price(unset), spot(unset));
    });
}

// =============================================================================
// Minting
// =============================================================================

#[test]
fn mint_gamma_values_at_the_lower_of_gamma_price_and_spot() {
    // Gamma price below spot, then far above it.
    for multiple in [0.5f64, 100.0] {
        new_test_ext(1).execute_with(|| {
            let netuid = gamma_subnet();
            let (coldkey, hotkey) = (U256::from(COLDKEY), U256::from(OWNER_HOTKEY));
            add_balance_to_coldkey_account(&coldkey, (11 * TAO).into());
            assert_ok!(SubtensorModule::add_stake(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                netuid,
                (10 * TAO).into()
            ));
            let price = SubnetGammaPrice::<Test>::get(netuid) * U64F64::from_num(multiple);
            SubnetGammaPrice::<Test>::insert(netuid, price);

            let alpha = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey, &coldkey, netuid,
            );
            let mark = price.min(<Test as Config>::SwapInterface::current_alpha_price(netuid));
            let value = (mark * U64F64::from_num(alpha.to_u64())).to_num::<u64>();
            let gamma = (u128::from(value) * u128::from(PRICE) / u128::from(TAO)) as u64;

            assert_ok!(SubtensorModule::mint_gamma_with_stake(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                U256::from(HOTKEY),
                netuid,
                alpha,
                GammaBalance::ZERO,
            ));
            System::assert_has_event(
                Event::GammaMinted {
                    netuid,
                    coldkey,
                    hotkey: U256::from(HOTKEY),
                    spender: U256::from(OWNER_COLDKEY),
                    tao: TaoBalance::ZERO,
                    alpha,
                    value: value.into(),
                    gamma: gamma.into(),
                }
                .into(),
            );
            assert_eq!(deposit(netuid, HOTKEY).balance, gamma.into());
            assert_gamma_invariants(netuid);
        });
    }
}

#[test]
fn mint_gamma_values_alpha_at_the_pre_trade_gamma_price() {
    new_test_ext(1).execute_with(|| {
        let netuid = add_dynamic_network(&U256::from(OWNER_HOTKEY), &U256::from(OWNER_COLDKEY));
        enable_gamma(netuid, 10_000 * TAO, 1_000_000 * TAO);
        let price = SubnetGammaPrice::<Test>::get(netuid);
        let coldkey = U256::from(COLDKEY);

        // Stake 10,000 TAO, then mint with 1,000 TAO in the same block.
        add_balance_to_coldkey_account(&coldkey, (11_001 * TAO).into());
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(coldkey),
            U256::from(OWNER_HOTKEY),
            netuid,
            (10_000 * TAO).into()
        ));
        mint(netuid, COLDKEY, HOTKEY, 1_000 * TAO);

        let (alpha, value, _) = minted();
        assert_eq!(
            value.to_u64(),
            (price * U64F64::from_num(alpha.to_u64())).to_num::<u64>()
        );
        assert_gamma_invariants(netuid);
    });
}

#[test]
fn mints_ignore_a_spot_rise_held_across_a_block() {
    new_test_ext(1).execute_with(|| {
        let netuid = add_dynamic_network(&U256::from(OWNER_HOTKEY), &U256::from(OWNER_COLDKEY));
        enable_gamma(netuid, 10_000 * TAO, 1_000_000 * TAO);
        let (coldkey, staker, hotkey) = (
            U256::from(COLDKEY),
            U256::from(COLDKEY + 1),
            U256::from(OWNER_HOTKEY),
        );
        add_balance_to_coldkey_account(&coldkey, (1_001 * TAO).into());
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            netuid,
            (1_000 * TAO).into()
        ));
        step_block(1);
        let stake =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid);
        let quote = || {
            SubtensorModule::sim_mint_gamma_with_stake(netuid, stake)
                .unwrap()
                .2
        };
        let before = quote().to_u64();

        // Raise spot and hold it into the next block.
        add_balance_to_coldkey_account(&staker, (10_001 * TAO).into());
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(staker),
            hotkey,
            netuid,
            (10_000 * TAO).into()
        ));
        step_block(1);
        assert!(u128::from(quote().to_u64()) * 100 <= u128::from(before) * 101);

        // Sold back, a mint with TAO values at the spot it buys from.
        let stake =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &staker, netuid);
        assert_ok!(SubtensorModule::remove_stake(
            RuntimeOrigin::signed(staker),
            hotkey,
            netuid,
            stake
        ));
        let (_, value, _) = SubtensorModule::sim_mint_gamma(netuid, (1_000 * TAO).into()).unwrap();
        assert!(value.to_u64() < 1_000 * TAO);
    });
}

#[test]
fn mint_gamma_with_stake_ignores_an_in_block_spot_rise() {
    let value = |raised: bool| {
        new_test_ext(1).execute_with(|| {
            let netuid = add_dynamic_network(&U256::from(OWNER_HOTKEY), &U256::from(OWNER_COLDKEY));
            enable_gamma(netuid, 10_000 * TAO, 1_000_000 * TAO);
            let (coldkey, hotkey) = (U256::from(COLDKEY), U256::from(OWNER_HOTKEY));
            // A position about as deep as the pool, held until the gamma price settles.
            add_balance_to_coldkey_account(&coldkey, (10_001 * TAO).into());
            assert_ok!(SubtensorModule::add_stake(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                netuid,
                (10_000 * TAO).into()
            ));
            step_block(1);
            SubnetGammaPrice::<Test>::insert(
                netuid,
                <Test as Config>::SwapInterface::current_alpha_price(netuid),
            );
            if raised {
                let staker = U256::from(77);
                add_balance_to_coldkey_account(&staker, (200_001 * TAO).into());
                assert_ok!(SubtensorModule::add_stake(
                    RuntimeOrigin::signed(staker),
                    hotkey,
                    netuid,
                    (200_000 * TAO).into()
                ));
            }
            let stake = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey, &coldkey, netuid,
            );
            assert_ok!(SubtensorModule::mint_gamma_with_stake(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                U256::from(HOTKEY),
                netuid,
                stake,
                GammaBalance::ZERO,
            ));
            minted().1.to_u64()
        })
    };
    let unraised = value(false);
    assert!(unraised > 0);
    assert!(value(true) <= unraised);
}

#[test]
fn mint_gamma_matches_add_stake_burn_totals() {
    let run = |mint_gamma: bool| {
        new_test_ext(1).execute_with(|| {
            let netuid = gamma_subnet();
            let coldkey = U256::from(COLDKEY);
            add_balance_to_coldkey_account(&coldkey, (101 * TAO).into());
            let author = U256::from(MOCK_BLOCK_BUILDER);
            let author_before = SubtensorModule::get_coldkey_balance(&author);
            let flow_before = SubnetTaoFlow::<Test>::get(netuid);
            System::reset_events();
            if mint_gamma {
                assert_ok!(SubtensorModule::mint_gamma(
                    RuntimeOrigin::signed(coldkey),
                    U256::from(HOTKEY),
                    netuid,
                    (100 * TAO).into(),
                    GammaBalance::ZERO,
                ));
                assert!(StakingHotkeys::<Test>::get(coldkey).is_empty());
                assert_eq!(
                    SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                        &U256::from(OWNER_HOTKEY),
                        &coldkey,
                        netuid
                    ),
                    AlphaBalance::ZERO
                );
                assert!(!System::events().iter().any(|record| matches!(
                    record.event,
                    RuntimeEvent::SubtensorModule(Event::StakeAdded(..) | Event::AlphaBurned(..))
                )));
                assert_gamma_invariants(netuid);
            } else {
                assert_ok!(SubtensorModule::add_stake_burn(
                    RuntimeOrigin::signed(coldkey),
                    U256::from(OWNER_HOTKEY),
                    netuid,
                    (100 * TAO).into(),
                    None,
                ));
            }
            (
                SubnetTAO::<Test>::get(netuid),
                SubnetAlphaIn::<Test>::get(netuid),
                SubnetAlphaOut::<Test>::get(netuid),
                TotalStake::<Test>::get(),
                SubnetVolume::<Test>::get(netuid),
                pallet_alpha_assets::AlphaBurned::<Test>::get(netuid),
                SubnetTaoFlow::<Test>::get(netuid) - flow_before,
                SubtensorModule::get_coldkey_balance(&author) - author_before,
                SubtensorModule::get_coldkey_balance(&coldkey),
            )
        })
    };
    assert_eq!(run(true), run(false));
}

#[test]
fn mint_gamma_with_stake_burns_as_burn_alpha() {
    let run = |mint_gamma: bool| {
        new_test_ext(1).execute_with(|| {
            let netuid = gamma_subnet();
            let (coldkey, hotkey) = (U256::from(COLDKEY), U256::from(OWNER_HOTKEY));
            add_balance_to_coldkey_account(&coldkey, (11 * TAO).into());
            assert_ok!(SubtensorModule::add_stake(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                netuid,
                (10 * TAO).into()
            ));
            let stake = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hotkey, &coldkey, netuid,
            );
            // Half the stake, then more than what is left, which burns the rest.
            for amount in [stake / 2.into(), AlphaBalance::MAX] {
                let staked = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                    &hotkey, &coldkey, netuid,
                );
                let burned = amount.min(staked);
                if mint_gamma {
                    assert_ok!(SubtensorModule::mint_gamma_with_stake(
                        RuntimeOrigin::signed(coldkey),
                        hotkey,
                        U256::from(HOTKEY),
                        netuid,
                        amount,
                        GammaBalance::ZERO,
                    ));
                } else {
                    assert_ok!(SubtensorModule::burn_alpha(
                        RuntimeOrigin::signed(coldkey),
                        hotkey,
                        amount,
                        netuid,
                    ));
                }
                System::assert_has_event(
                    Event::AlphaBurned(coldkey, hotkey, burned, netuid).into(),
                );
            }
            if mint_gamma {
                assert_eq!(
                    ColdkeyGammaHotkeys::<Test>::get(netuid, coldkey).into_inner(),
                    vec![U256::from(HOTKEY)]
                );
                assert_gamma_invariants(netuid);
            }
            (
                SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                    &hotkey, &coldkey, netuid,
                ),
                SubnetTAO::<Test>::get(netuid),
                SubnetAlphaIn::<Test>::get(netuid),
                SubnetAlphaOut::<Test>::get(netuid),
                TotalStake::<Test>::get(),
                pallet_alpha_assets::AlphaBurned::<Test>::get(netuid),
            )
        })
    };
    let burned = run(false);
    assert_eq!(burned.0, AlphaBalance::ZERO);
    assert_eq!(run(true), burned);
}

#[test]
fn mint_gamma_opens_and_tops_up_deposits() {
    new_test_ext(1).execute_with(|| {
        let netuid = gamma_subnet();
        let (coldkey, hotkey) = (U256::from(COLDKEY), U256::from(HOTKEY));
        let gamma = mint(netuid, COLDKEY, HOTKEY, TAO);
        assert_eq!(
            deposit(netuid, HOTKEY),
            GammaDeposit {
                coldkey,
                spender: U256::from(OWNER_COLDKEY),
                balance: gamma,
                daily_spend_cap: None,
                bucket: (GammaBalance::ZERO, System::block_number()),
            }
        );

        // A top-up keeps the deposit's terms.
        assert_ok!(SubtensorModule::set_gamma_deposit_spender(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            netuid,
            U256::from(77),
        ));
        assert_ok!(SubtensorModule::set_gamma_deposit_daily_spend_cap(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            netuid,
            Some(GAMMA.into()),
        ));
        let terms = deposit(netuid, HOTKEY);
        let more = mint(netuid, COLDKEY, HOTKEY, TAO);
        assert_eq!(
            deposit(netuid, HOTKEY),
            GammaDeposit {
                balance: gamma.saturating_add(more),
                ..terms
            }
        );

        // Another coldkey cannot mint to the hotkey.
        add_balance_to_coldkey_account(&U256::from(COLDKEY + 100), (2 * TAO).into());
        assert_noop!(
            SubtensorModule::mint_gamma(
                RuntimeOrigin::signed(U256::from(COLDKEY + 100)),
                hotkey,
                netuid,
                TAO.into(),
                GammaBalance::ZERO,
            ),
            Error::<Test>::NonAssociatedColdKey
        );

        // A coldkey holds at most `MAX_GAMMA_DEPOSITS` deposits per subnet.
        for index in 1..MAX_GAMMA_DEPOSITS as u64 {
            mint(netuid, COLDKEY, HOTKEY + 1_000 + index, TAO);
        }
        assert_eq!(
            ColdkeyGammaHotkeys::<Test>::get(netuid, coldkey).len(),
            MAX_GAMMA_DEPOSITS as usize
        );
        add_balance_to_coldkey_account(&coldkey, (2 * TAO).into());
        assert_noop!(
            SubtensorModule::mint_gamma(
                RuntimeOrigin::signed(coldkey),
                U256::from(HOTKEY + 2_000),
                netuid,
                TAO.into(),
                GammaBalance::ZERO,
            ),
            Error::<Test>::TooManyGammaDeposits
        );
        assert_gamma_invariants(netuid);
    });
}

#[test]
fn mint_gamma_credits_at_least_the_minimum() {
    new_test_ext(1).execute_with(|| {
        let netuid = gamma_subnet();
        let (coldkey, hotkey) = (U256::from(COLDKEY), U256::from(OWNER_HOTKEY));
        add_balance_to_coldkey_account(&coldkey, (2 * TAO).into());
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            netuid,
            TAO.into()
        ));
        let alpha =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid);
        let (value, _) =
            SubtensorModule::gamma_for_alpha(alpha, SubtensorModule::gamma_mark(netuid)).unwrap();
        // The lowest price at which the alpha mints `MIN_GAMMA_AMOUNT`.
        let price = (u128::from(MIN_GAMMA_AMOUNT) * u128::from(TAO))
            .div_ceil(u128::from(value.to_u64())) as u64;
        let fed = |price: u64| {
            TaoUsdPrice::<Test>::put(TaoUsdPriceState {
                accepted: price,
                feed: price,
                fed_at: System::block_number(),
            });
        };
        let mint = || {
            SubtensorModule::mint_gamma_with_stake(
                RuntimeOrigin::signed(coldkey),
                hotkey,
                U256::from(HOTKEY),
                netuid,
                alpha,
                GammaBalance::ZERO,
            )
        };
        fed(price - 1);
        assert_noop!(mint(), Error::<Test>::AmountTooLow);
        fed(price);
        assert_ok!(mint());
        assert_eq!(deposit(netuid, HOTKEY).balance, MIN_GAMMA_AMOUNT.into());
    });
}

#[test]
fn mint_gamma_refusals() {
    new_test_ext(1).execute_with(|| {
        let netuid = gamma_subnet();
        let coldkey = U256::from(COLDKEY);
        add_balance_to_coldkey_account(&coldkey, (12 * TAO).into());
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(coldkey),
            U256::from(OWNER_HOTKEY),
            netuid,
            TAO.into()
        ));
        let mint = |netuid: NetUid, amount: u64, min_amount_out: u64| {
            SubtensorModule::mint_gamma(
                RuntimeOrigin::signed(coldkey),
                U256::from(HOTKEY),
                netuid,
                amount.into(),
                min_amount_out.into(),
            )
        };
        let mint_with_stake = |origin_hotkey: u64, destination_hotkey: u64, min_amount_out: u64| {
            SubtensorModule::mint_gamma_with_stake(
                RuntimeOrigin::signed(coldkey),
                U256::from(origin_hotkey),
                U256::from(destination_hotkey),
                netuid,
                AlphaBalance::MAX,
                min_amount_out.into(),
            )
        };
        let with_stake = || mint_with_stake(OWNER_HOTKEY, HOTKEY, 0);

        GammaEnabled::<Test>::put(false);
        assert_noop!(mint(netuid, TAO, 0), Error::<Test>::GammaDisabled);
        assert_noop!(with_stake(), Error::<Test>::GammaDisabled);
        GammaEnabled::<Test>::put(true);
        SubnetGammaEnabled::<Test>::insert(netuid, false);
        assert_noop!(mint(netuid, TAO, 0), Error::<Test>::GammaDisabled);
        assert_noop!(with_stake(), Error::<Test>::GammaDisabled);
        SubnetGammaEnabled::<Test>::insert(netuid, true);
        SubtokenEnabled::<Test>::insert(netuid, false);
        assert_noop!(mint(netuid, TAO, 0), Error::<Test>::SubtokenDisabled);
        assert_noop!(with_stake(), Error::<Test>::SubtokenDisabled);
        SubtokenEnabled::<Test>::insert(netuid, true);

        let price = SubnetGammaPrice::<Test>::take(netuid);
        assert_noop!(mint(netuid, TAO, 0), Error::<Test>::AmountTooLow);
        assert_noop!(with_stake(), Error::<Test>::AmountTooLow);
        SubnetGammaPrice::<Test>::insert(netuid, price);

        let price = TaoUsdPrice::<Test>::take();
        assert_noop!(mint(netuid, TAO, 0), Error::<Test>::TaoUsdPriceUnavailable);
        assert_noop!(with_stake(), Error::<Test>::TaoUsdPriceUnavailable);
        TaoUsdPrice::<Test>::set(price);

        assert_noop!(mint(netuid, 100_000, 0), Error::<Test>::AmountTooLow);
        assert_noop!(mint(netuid, TAO, u64::MAX), Error::<Test>::SlippageTooHigh);
        assert_noop!(
            mint_with_stake(OWNER_HOTKEY, HOTKEY, u64::MAX),
            Error::<Test>::SlippageTooHigh
        );
        assert_noop!(
            mint_with_stake(HOTKEY, HOTKEY, 0),
            Error::<Test>::HotKeyAccountNotExists
        );
        assert_noop!(
            mint(netuid, 100 * TAO, 0),
            Error::<Test>::NotEnoughBalanceToStake
        );
        let over_limit = SubnetTAO::<Test>::get(netuid).to_u64() * 1_000 + 1;
        add_balance_to_coldkey_account(&coldkey, over_limit.into());
        assert_noop!(
            mint(netuid, over_limit, 0),
            Error::<Test>::InsufficientLiquidity
        );
        assert_noop!(
            mint(NetUid::from(99), TAO, 0),
            Error::<Test>::SubnetNotExists
        );

        let foreign = HOTKEY + 1;
        add_balance_to_coldkey_account(&U256::from(OWNER_COLDKEY), (2 * TAO).into());
        assert_ok!(SubtensorModule::mint_gamma(
            RuntimeOrigin::signed(U256::from(OWNER_COLDKEY)),
            U256::from(foreign),
            netuid,
            TAO.into(),
            GammaBalance::ZERO,
        ));
        assert_noop!(
            SubtensorModule::mint_gamma(
                RuntimeOrigin::signed(coldkey),
                U256::from(foreign),
                netuid,
                TAO.into(),
                GammaBalance::ZERO,
            ),
            Error::<Test>::NonAssociatedColdKey
        );
        assert_noop!(
            mint_with_stake(OWNER_HOTKEY, foreign, 0),
            Error::<Test>::NonAssociatedColdKey
        );
    });
}

// =============================================================================
// Deposits
// =============================================================================

#[test]
fn move_gamma_moves_up_to_the_amount() {
    new_test_ext(1).execute_with(|| {
        let netuid = gamma_subnet();
        let coldkey = U256::from(COLDKEY);
        let signed = || RuntimeOrigin::signed(coldkey);
        let gamma = mint(netuid, COLDKEY, HOTKEY, TAO);
        let (spender, cap) = (U256::from(77), Some(GammaBalance::from(GAMMA)));
        assert_ok!(SubtensorModule::set_gamma_deposit_spender(
            signed(),
            U256::from(HOTKEY),
            netuid,
            spender
        ));
        assert_ok!(SubtensorModule::set_gamma_deposit_daily_spend_cap(
            signed(),
            U256::from(HOTKEY),
            netuid,
            cap
        ));
        let other = mint(netuid, COLDKEY, HOTKEY + 2, TAO);
        System::set_block_number(System::block_number() + 100);
        let now = System::block_number();

        // Part of it opens a deposit with the origin's terms and a full bucket.
        let part = GammaBalance::from(gamma.to_u64() / 4);
        assert_ok!(SubtensorModule::move_gamma(
            signed(),
            U256::from(HOTKEY),
            U256::from(HOTKEY + 1),
            netuid,
            part
        ));
        assert_eq!(
            deposit(netuid, HOTKEY + 1),
            GammaDeposit {
                coldkey,
                spender,
                balance: part,
                daily_spend_cap: cap,
                bucket: (GAMMA.into(), now),
            }
        );
        assert_eq!(deposit(netuid, HOTKEY).balance, gamma.saturating_sub(part));
        System::assert_has_event(
            Event::GammaMoved {
                netuid,
                coldkey,
                origin_hotkey: U256::from(HOTKEY),
                destination_hotkey: U256::from(HOTKEY + 1),
                amount: part,
            }
            .into(),
        );

        // Into an existing deposit, which keeps its own terms.
        let terms = deposit(netuid, HOTKEY + 2);
        assert_ok!(SubtensorModule::move_gamma(
            signed(),
            U256::from(HOTKEY + 1),
            U256::from(HOTKEY + 2),
            netuid,
            GammaBalance::from(1)
        ));
        assert_eq!(
            deposit(netuid, HOTKEY + 2),
            GammaDeposit {
                balance: other.saturating_add(1.into()),
                ..terms
            }
        );

        // More than the balance moves all of it and closes the origin.
        assert_ok!(SubtensorModule::move_gamma(
            signed(),
            U256::from(HOTKEY),
            U256::from(HOTKEY + 2),
            netuid,
            GammaBalance::MAX
        ));
        assert_eq!(GammaDeposits::<Test>::get(netuid, U256::from(HOTKEY)), None);
        assert!(!ColdkeyGammaHotkeys::<Test>::get(netuid, coldkey).contains(&U256::from(HOTKEY)));
        System::assert_has_event(
            Event::GammaMoved {
                netuid,
                coldkey,
                origin_hotkey: U256::from(HOTKEY),
                destination_hotkey: U256::from(HOTKEY + 2),
                amount: gamma.saturating_sub(part),
            }
            .into(),
        );
        System::assert_has_event(
            Event::GammaDepositClosed {
                netuid,
                coldkey,
                hotkey: U256::from(HOTKEY),
            }
            .into(),
        );
        assert_gamma_invariants(netuid);
    });
}

#[test]
fn move_gamma_refusals() {
    new_test_ext(1).execute_with(|| {
        let netuid = gamma_subnet();
        let signed = |coldkey: u64| RuntimeOrigin::signed(U256::from(coldkey));
        let (stranger, theirs) = (COLDKEY + 100, HOTKEY + 100);
        mint(netuid, COLDKEY, HOTKEY, TAO);
        mint(netuid, stranger, theirs, TAO);
        let move_gamma = |coldkey: u64, from: u64, to: u64, amount: u64| {
            SubtensorModule::move_gamma(
                signed(coldkey),
                U256::from(from),
                U256::from(to),
                netuid,
                amount.into(),
            )
        };

        assert_noop!(
            move_gamma(COLDKEY, HOTKEY, HOTKEY, GAMMA),
            Error::<Test>::InvalidValue
        );
        assert_noop!(
            move_gamma(COLDKEY, HOTKEY, HOTKEY + 1, 0),
            Error::<Test>::AmountTooLow
        );
        assert_noop!(
            move_gamma(COLDKEY, HOTKEY + 1, HOTKEY, GAMMA),
            Error::<Test>::GammaDepositDoesNotExist
        );
        assert_noop!(
            move_gamma(COLDKEY, theirs, HOTKEY, GAMMA),
            Error::<Test>::NonAssociatedColdKey
        );
        assert_noop!(
            move_gamma(COLDKEY, HOTKEY, theirs, GAMMA),
            Error::<Test>::NonAssociatedColdKey
        );
        assert_noop!(
            SubtensorModule::move_gamma(
                signed(COLDKEY),
                U256::from(HOTKEY),
                U256::from(HOTKEY + 1),
                NetUid::from(99),
                GAMMA.into()
            ),
            Error::<Test>::SubnetNotExists
        );

        // The setters refuse the same deposits.
        for hotkey in [HOTKEY + 1, theirs] {
            let error = || {
                if hotkey == theirs {
                    Error::<Test>::NonAssociatedColdKey
                } else {
                    Error::<Test>::GammaDepositDoesNotExist
                }
            };
            assert_noop!(
                SubtensorModule::set_gamma_deposit_spender(
                    signed(COLDKEY),
                    U256::from(hotkey),
                    netuid,
                    U256::from(77)
                ),
                error()
            );
            assert_noop!(
                SubtensorModule::set_gamma_deposit_daily_spend_cap(
                    signed(COLDKEY),
                    U256::from(hotkey),
                    netuid,
                    None
                ),
                error()
            );
            assert_noop!(
                SubtensorModule::close_gamma_deposit(signed(COLDKEY), U256::from(hotkey), netuid),
                error()
            );
        }

        // A move cannot open a deposit past `MAX_GAMMA_DEPOSITS`.
        for index in 1..MAX_GAMMA_DEPOSITS as u64 {
            mint(netuid, COLDKEY, HOTKEY + 1_000 + index, TAO);
        }
        assert_noop!(
            move_gamma(COLDKEY, HOTKEY, HOTKEY + 1, GAMMA),
            Error::<Test>::TooManyGammaDeposits
        );
        assert_gamma_invariants(netuid);
    });
}

#[test]
fn close_gamma_deposit_releases_the_hotkey() {
    new_test_ext(1).execute_with(|| {
        let netuid = gamma_subnet();
        let (coldkey, hotkey) = (U256::from(COLDKEY), U256::from(HOTKEY));
        let gamma = mint(netuid, COLDKEY, HOTKEY, TAO);
        mint(netuid, COLDKEY, HOTKEY + 1, TAO);
        assert_noop!(
            SubtensorModule::close_gamma_deposit(RuntimeOrigin::signed(coldkey), hotkey, netuid),
            Error::<Test>::GammaDepositNotEmpty
        );

        // Spent to zero, the deposit stays open until closed.
        assert_ok!(SubtensorModule::spend_gamma(
            RuntimeOrigin::signed(U256::from(OWNER_COLDKEY)),
            netuid,
            spends(&[(HOTKEY, gamma.to_u64())])
        ));
        assert_eq!(deposit(netuid, HOTKEY).balance, GammaBalance::ZERO);
        assert_ok!(SubtensorModule::close_gamma_deposit(
            RuntimeOrigin::signed(coldkey),
            hotkey,
            netuid
        ));
        System::assert_has_event(
            Event::GammaDepositClosed {
                netuid,
                coldkey,
                hotkey,
            }
            .into(),
        );
        assert_eq!(GammaDeposits::<Test>::get(netuid, hotkey), None);
        assert_eq!(
            ColdkeyGammaHotkeys::<Test>::get(netuid, coldkey).into_inner(),
            vec![U256::from(HOTKEY + 1)]
        );

        // Another coldkey can now open a deposit at the hotkey.
        mint(netuid, COLDKEY + 100, HOTKEY, TAO);
        assert_eq!(deposit(netuid, HOTKEY).coldkey, U256::from(COLDKEY + 100));
        assert_gamma_invariants(netuid);
    });
}

#[test]
fn daily_spend_cap_bucket_rules() {
    new_test_ext(1).execute_with(|| {
        let netuid = gamma_subnet();
        let coldkey = U256::from(COLDKEY);
        mint(netuid, COLDKEY, HOTKEY, 10 * TAO);
        let cap = |cap: Option<u64>| {
            assert_ok!(SubtensorModule::set_gamma_deposit_daily_spend_cap(
                RuntimeOrigin::signed(coldkey),
                U256::from(HOTKEY),
                netuid,
                cap.map(GammaBalance::from),
            ));
        };
        let spendable = || {
            SubtensorModule::get_gamma_deposit(netuid, U256::from(HOTKEY))
                .unwrap()
                .spendable
                .to_u64()
        };
        let spend = |amount: u64| {
            assert_ok!(SubtensorModule::spend_gamma(
                RuntimeOrigin::signed(U256::from(OWNER_COLDKEY)),
                netuid,
                spends(&[(HOTKEY, amount)])
            ));
        };
        let wait = |blocks: u64| System::set_block_number(System::block_number() + blocks);
        let day = GAMMA_REFILL_BLOCKS;

        // A new cap starts full, and refills evenly over a day up to the cap.
        assert_eq!(spendable(), deposit(netuid, HOTKEY).balance.to_u64());
        cap(Some(100 * GAMMA));
        System::assert_has_event(
            Event::GammaDepositDailySpendCapSet {
                netuid,
                coldkey,
                hotkey: U256::from(HOTKEY),
                daily_spend_cap: Some((100 * GAMMA).into()),
            }
            .into(),
        );
        assert_eq!(spendable(), 100 * GAMMA);
        spend(u64::MAX);
        assert_eq!(spendable(), 0);
        wait(day / 4);
        assert_eq!(spendable(), 25 * GAMMA);
        wait(day);
        assert_eq!(spendable(), 100 * GAMMA);

        // A full bucket plus a day's refill spends two caps in one day.
        spend(100 * GAMMA);
        wait(day);
        spend(100 * GAMMA);
        assert_eq!(spendable(), 0);

        // Raising adds the increase; lowering clamps.
        wait(day / 2);
        cap(Some(200 * GAMMA));
        assert_eq!(spendable(), 150 * GAMMA);
        cap(Some(40 * GAMMA));
        assert_eq!(spendable(), 40 * GAMMA);

        // Zero pauses the deposit: an entry spends nothing and emits nothing.
        cap(Some(0));
        wait(day);
        System::reset_events();
        spend(GAMMA);
        assert!(gamma_events().is_empty());

        // `None` removes the cap.
        cap(None);
        assert_eq!(spendable(), deposit(netuid, HOTKEY).balance.to_u64());
        assert_gamma_invariants(netuid);
    });
}

// =============================================================================
// Spending
// =============================================================================

#[test]
fn spend_gamma_spends_the_smallest_of_amount_balance_and_allowance() {
    new_test_ext(1).execute_with(|| {
        let netuid = gamma_subnet();
        let owner = RuntimeOrigin::signed(U256::from(OWNER_COLDKEY));
        let coldkey = U256::from(COLDKEY);
        let gamma = mint(netuid, COLDKEY, HOTKEY, TAO).to_u64();
        mint(netuid, COLDKEY, HOTKEY + 1, TAO);
        assert_ok!(SubtensorModule::set_gamma_deposit_daily_spend_cap(
            RuntimeOrigin::signed(coldkey),
            U256::from(HOTKEY + 1),
            netuid,
            Some((2 * GAMMA).into()),
        ));
        System::reset_events();

        // In order: the amount, then the rest of the balance, then nothing; the allowance
        // bounds the capped deposit across its two entries.
        assert_ok!(SubtensorModule::spend_gamma(
            owner,
            netuid,
            spends(&[
                (HOTKEY, GAMMA),
                (HOTKEY + 1, GAMMA + 1),
                (HOTKEY, u64::MAX),
                (HOTKEY + 1, u64::MAX),
                (HOTKEY, GAMMA),
            ])
        ));
        let spent = |hotkey: u64, amount: u64| Event::GammaSpent {
            netuid,
            coldkey,
            hotkey: U256::from(hotkey),
            amount: amount.into(),
        };
        assert_eq!(
            gamma_events(),
            vec![
                spent(HOTKEY, GAMMA),
                spent(HOTKEY + 1, GAMMA + 1),
                spent(HOTKEY, gamma - GAMMA),
                spent(HOTKEY + 1, GAMMA - 1),
            ]
        );
        assert!(deposit(netuid, HOTKEY).balance.is_zero());
        assert_eq!(
            deposit(netuid, HOTKEY + 1).bucket,
            (GammaBalance::ZERO, System::block_number())
        );
        assert_gamma_invariants(netuid);
    });
}

#[test]
fn spend_gamma_skips_silently() {
    new_test_ext(1).execute_with(|| {
        let netuid = gamma_subnet();
        let owner = U256::from(OWNER_COLDKEY);
        mint(netuid, COLDKEY, HOTKEY, TAO);
        let before = deposit(netuid, HOTKEY);
        System::reset_events();

        assert_ok!(SubtensorModule::spend_gamma(
            RuntimeOrigin::signed(owner),
            netuid,
            spends(&[(HOTKEY + 1, GAMMA), (HOTKEY, 0)])
        ));
        assert_ok!(SubtensorModule::spend_gamma(
            RuntimeOrigin::signed(U256::from(COLDKEY)),
            netuid,
            spends(&[(HOTKEY, GAMMA)])
        ));
        assert!(gamma_events().is_empty());
        assert_eq!(deposit(netuid, HOTKEY), before);
    });
}

#[test]
fn spend_gamma_refusals() {
    new_test_ext(1).execute_with(|| {
        let netuid = gamma_subnet();
        let owner = U256::from(OWNER_COLDKEY);
        assert_noop!(
            SubtensorModule::spend_gamma(RuntimeOrigin::signed(owner), netuid, spends(&[])),
            Error::<Test>::InvalidValue
        );
        assert_noop!(
            SubtensorModule::spend_gamma(
                RuntimeOrigin::signed(owner),
                NetUid::from(99),
                spends(&[(HOTKEY, GAMMA)])
            ),
            Error::<Test>::SubnetNotExists
        );
    });
}

#[test]
fn deposits_open_with_the_subnet_owner_or_lease_beneficiary() {
    new_test_ext(1).execute_with(|| {
        let netuid = gamma_subnet();
        mint(netuid, COLDKEY, HOTKEY, TAO);
        let spend = |spender: u64| {
            let before = deposit(netuid, HOTKEY).balance;
            assert_ok!(SubtensorModule::spend_gamma(
                RuntimeOrigin::signed(U256::from(spender)),
                netuid,
                spends(&[(HOTKEY, 1)])
            ));
            before != deposit(netuid, HOTKEY).balance
        };
        assert!(spend(OWNER_COLDKEY));
        // A new owner spends only the deposits opened after it.
        SubnetOwner::<Test>::insert(netuid, U256::from(55));
        assert!(!spend(55));
        assert!(spend(OWNER_COLDKEY));
    });

    new_test_ext(1).execute_with(|| {
        let beneficiary = U256::from(1);
        super::leasing::setup_crowdloan(0, 10 * TAO, 1_000 * TAO, beneficiary, &[]);
        let (_, lease) = super::leasing::setup_leased_network(
            beneficiary,
            sp_runtime::Percent::from_percent(30),
            None,
            None,
        );
        FirstEmissionBlockNumber::<Test>::insert(lease.netuid, 0);
        enable_gamma(lease.netuid, 1_000 * TAO, 1_000 * TAO);
        assert_eq!(
            SubtensorModule::gamma_default_spender(lease.netuid),
            beneficiary
        );
        assert_ne!(SubnetOwner::<Test>::get(lease.netuid), beneficiary);
        mint(lease.netuid, COLDKEY, HOTKEY, TAO);
        let before = deposit(lease.netuid, HOTKEY).balance;
        assert_ok!(SubtensorModule::spend_gamma(
            RuntimeOrigin::signed(lease.coldkey),
            lease.netuid,
            spends(&[(HOTKEY, 1)])
        ));
        assert_eq!(deposit(lease.netuid, HOTKEY).balance, before);
        assert_ok!(SubtensorModule::spend_gamma(
            RuntimeOrigin::signed(beneficiary),
            lease.netuid,
            spends(&[(HOTKEY, 1)])
        ));
        assert_eq!(
            deposit(lease.netuid, HOTKEY).balance,
            before.saturating_sub(1.into())
        );
        assert_gamma_invariants(lease.netuid);

        // A conviction takeover ends the lease's hold on the subnet.
        SubnetOwner::<Test>::insert(lease.netuid, U256::from(55));
        assert_eq!(
            SubtensorModule::gamma_default_spender(lease.netuid),
            U256::from(55)
        );
    });
}

#[test]
fn named_spender_excludes_the_owner() {
    new_test_ext(1).execute_with(|| {
        let netuid = gamma_subnet();
        let spender = U256::from(77);
        mint(netuid, COLDKEY, HOTKEY, TAO);
        assert_ok!(SubtensorModule::set_gamma_deposit_spender(
            RuntimeOrigin::signed(U256::from(COLDKEY)),
            U256::from(HOTKEY),
            netuid,
            spender,
        ));
        System::assert_has_event(
            Event::GammaDepositSpenderSet {
                netuid,
                coldkey: U256::from(COLDKEY),
                hotkey: U256::from(HOTKEY),
                spender,
            }
            .into(),
        );
        let before = deposit(netuid, HOTKEY).balance;
        assert_ok!(SubtensorModule::spend_gamma(
            RuntimeOrigin::signed(U256::from(OWNER_COLDKEY)),
            netuid,
            spends(&[(HOTKEY, GAMMA)])
        ));
        assert_eq!(deposit(netuid, HOTKEY).balance, before);
        assert_ok!(SubtensorModule::spend_gamma(
            RuntimeOrigin::signed(spender),
            netuid,
            spends(&[(HOTKEY, GAMMA)])
        ));
        assert_eq!(
            deposit(netuid, HOTKEY).balance,
            before.saturating_sub(GAMMA.into())
        );
    });
}

#[test]
fn gamma_spend_proxy_spends_as_its_real_account() {
    new_test_ext(1).execute_with(|| {
        let netuid = gamma_subnet();
        let (owner, delegate) = (U256::from(OWNER_COLDKEY), U256::from(88));
        mint(netuid, COLDKEY, HOTKEY, TAO);
        add_balance_to_coldkey_account(&owner, TAO.into());
        assert_ok!(pallet_subtensor_proxy::Pallet::<Test>::add_proxy_delegate(
            &owner,
            delegate,
            ProxyType::GammaSpend,
            0
        ));
        let before = deposit(netuid, HOTKEY).balance;
        assert_ok!(
            RuntimeCall::Proxy(pallet_subtensor_proxy::Call::proxy {
                real: owner,
                force_proxy_type: Some(ProxyType::GammaSpend),
                call: Box::new(RuntimeCall::SubtensorModule(SubtensorCall::spend_gamma {
                    netuid,
                    spends: spends(&[(HOTKEY, GAMMA)]),
                })),
            })
            .dispatch(RuntimeOrigin::signed(delegate))
        );
        assert_eq!(
            deposit(netuid, HOTKEY).balance,
            before.saturating_sub(GAMMA.into())
        );
    });
}

/// Every deposit follows from the gamma events.
#[test]
fn gamma_events_reconstruct_deposits() {
    new_test_ext(1).execute_with(|| {
        let netuid = gamma_subnet();
        let coldkey = U256::from(COLDKEY);
        let signed = || RuntimeOrigin::signed(coldkey);
        System::reset_events();

        mint(netuid, COLDKEY, HOTKEY, 2 * TAO);
        mint(netuid, COLDKEY, HOTKEY + 1, TAO);
        assert_ok!(SubtensorModule::set_gamma_deposit_spender(
            signed(),
            U256::from(HOTKEY),
            netuid,
            U256::from(77)
        ));
        assert_ok!(SubtensorModule::set_gamma_deposit_daily_spend_cap(
            signed(),
            U256::from(HOTKEY),
            netuid,
            Some((5 * GAMMA).into())
        ));
        assert_ok!(SubtensorModule::move_gamma(
            signed(),
            U256::from(HOTKEY),
            U256::from(HOTKEY + 2),
            netuid,
            (50 * GAMMA).into()
        ));
        assert_ok!(SubtensorModule::spend_gamma(
            RuntimeOrigin::signed(U256::from(77)),
            netuid,
            spends(&[(HOTKEY, 3 * GAMMA), (HOTKEY + 2, GAMMA)])
        ));
        assert_ok!(SubtensorModule::spend_gamma(
            RuntimeOrigin::signed(U256::from(OWNER_COLDKEY)),
            netuid,
            spends(&[(HOTKEY + 1, u64::MAX)])
        ));
        assert_ok!(SubtensorModule::close_gamma_deposit(
            signed(),
            U256::from(HOTKEY + 1),
            netuid
        ));
        assert_ok!(SubtensorModule::move_gamma(
            signed(),
            U256::from(HOTKEY + 2),
            U256::from(HOTKEY),
            netuid,
            GammaBalance::MAX
        ));
        mint(netuid, COLDKEY, HOTKEY + 1, TAO);

        // (coldkey, spender, balance, daily spend cap) per hotkey.
        let mut rebuilt = BTreeMap::new();
        for event in gamma_events() {
            match event {
                Event::GammaMinted {
                    hotkey,
                    coldkey,
                    spender,
                    gamma,
                    ..
                } => {
                    rebuilt
                        .entry(hotkey)
                        .or_insert((coldkey, spender, GammaBalance::ZERO, None))
                        .2 += gamma;
                }
                Event::GammaMoved {
                    origin_hotkey,
                    destination_hotkey,
                    amount,
                    ..
                } => {
                    let origin = rebuilt.get_mut(&origin_hotkey).unwrap();
                    origin.2 -= amount;
                    let terms = (origin.0, origin.1, GammaBalance::ZERO, origin.3);
                    rebuilt.entry(destination_hotkey).or_insert(terms).2 += amount;
                }
                Event::GammaDepositClosed { hotkey, .. } => {
                    rebuilt.remove(&hotkey);
                }
                Event::GammaDepositSpenderSet {
                    hotkey, spender, ..
                } => {
                    rebuilt.get_mut(&hotkey).unwrap().1 = spender;
                }
                Event::GammaDepositDailySpendCapSet {
                    hotkey,
                    daily_spend_cap,
                    ..
                } => {
                    rebuilt.get_mut(&hotkey).unwrap().3 = daily_spend_cap;
                }
                Event::GammaSpent { hotkey, amount, .. } => {
                    rebuilt.get_mut(&hotkey).unwrap().2 -= amount;
                }
                _ => unreachable!(),
            }
        }
        let stored: BTreeMap<_, _> = GammaDeposits::<Test>::iter_prefix(netuid)
            .map(|(hotkey, deposit)| {
                (
                    hotkey,
                    (
                        deposit.coldkey,
                        deposit.spender,
                        deposit.balance,
                        deposit.daily_spend_cap,
                    ),
                )
            })
            .collect();
        assert_eq!(rebuilt, stored);
        assert_gamma_invariants(netuid);
    });
}

// =============================================================================
// Swaps
// =============================================================================

#[test]
fn announced_coldkey_swap_blocks_gamma_calls_but_not_other_spenders() {
    new_test_ext(1).execute_with(|| {
        let netuid = gamma_subnet();
        let (coldkey, spender) = (U256::from(COLDKEY), U256::from(77));
        mint(netuid, COLDKEY, HOTKEY, TAO);
        mint(netuid, COLDKEY, HOTKEY + 1, TAO);
        assert_ok!(SubtensorModule::set_gamma_deposit_spender(
            RuntimeOrigin::signed(coldkey),
            U256::from(HOTKEY),
            netuid,
            spender,
        ));
        add_balance_to_coldkey_account(&coldkey, (2 * TAO).into());
        let dispatch = |who: U256, call: SubtensorCall| {
            <CheckColdkeySwap<Test> as ExtendedDispatchable<RuntimeCall>>::dispatch_with_extension(
                RuntimeOrigin::signed(who),
                RuntimeCall::SubtensorModule(call),
            )
            .map_err(|error| error.error)
        };
        let announce = |who: U256| {
            ColdkeySwapAnnouncements::<Test>::insert(
                who,
                (System::block_number(), sp_core::H256::zero()),
            );
        };
        let spend = |hotkey: u64| SubtensorCall::spend_gamma {
            netuid,
            spends: spends(&[(hotkey, 1)]),
        };
        let announced = Err(Error::<Test>::ColdkeySwapAnnounced.into());

        announce(coldkey);
        for call in [
            SubtensorCall::mint_gamma {
                hotkey: U256::from(HOTKEY),
                netuid,
                amount: TAO.into(),
                min_amount_out: GammaBalance::ZERO,
            },
            SubtensorCall::move_gamma {
                origin_hotkey: U256::from(HOTKEY),
                destination_hotkey: U256::from(HOTKEY + 2),
                netuid,
                amount: GAMMA.into(),
            },
            SubtensorCall::set_gamma_deposit_spender {
                hotkey: U256::from(HOTKEY),
                netuid,
                spender: U256::from(78),
            },
        ] {
            assert_eq!(dispatch(coldkey, call), announced);
        }

        // Its spenders still spend its deposits.
        let before = deposit(netuid, HOTKEY).balance;
        assert_ok!(dispatch(spender, spend(HOTKEY)));
        assert_eq!(
            deposit(netuid, HOTKEY).balance,
            before.saturating_sub(1.into())
        );

        // A spender whose own swap is announced cannot spend.
        announce(spender);
        assert_eq!(dispatch(spender, spend(HOTKEY)), announced);
        announce(U256::from(OWNER_COLDKEY));
        assert_eq!(
            dispatch(U256::from(OWNER_COLDKEY), spend(HOTKEY + 1)),
            announced
        );
    });
}

#[test]
fn hotkey_swap_leaves_gamma_deposits() {
    new_test_ext(1).execute_with(|| {
        let netuid = gamma_subnet();
        let (owner_hotkey, new_hotkey) = (U256::from(OWNER_HOTKEY), U256::from(99));
        mint(netuid, OWNER_COLDKEY, OWNER_HOTKEY, TAO);
        let before = deposit(netuid, OWNER_HOTKEY);
        add_balance_to_coldkey_account(&U256::from(OWNER_COLDKEY), (10 * TAO).into());
        System::set_block_number(System::block_number() + 1_000);
        assert_ok!(SubtensorModule::swap_hotkey_v2(
            RuntimeOrigin::signed(U256::from(OWNER_COLDKEY)),
            owner_hotkey,
            new_hotkey,
            None,
            false
        ));
        assert_eq!(deposit(netuid, OWNER_HOTKEY), before);
        assert_eq!(GammaDeposits::<Test>::get(netuid, new_hotkey), None);
        assert_gamma_invariants(netuid);
    });
}

#[test]
fn exits_stay_open_with_minting_off() {
    new_test_ext(1).execute_with(|| {
        let netuid = gamma_subnet();
        let coldkey = RuntimeOrigin::signed(U256::from(COLDKEY));
        mint(netuid, COLDKEY, HOTKEY, TAO);
        SubnetGammaEnabled::<Test>::insert(netuid, false);
        SubtokenEnabled::<Test>::insert(netuid, false);
        TaoUsdPrice::<Test>::kill();

        assert_ok!(SubtensorModule::move_gamma(
            coldkey.clone(),
            U256::from(HOTKEY),
            U256::from(HOTKEY + 1),
            netuid,
            GAMMA.into()
        ));
        assert_ok!(SubtensorModule::set_gamma_deposit_daily_spend_cap(
            coldkey.clone(),
            U256::from(HOTKEY + 1),
            netuid,
            Some(GAMMA.into())
        ));
        assert_ok!(SubtensorModule::set_gamma_deposit_spender(
            coldkey.clone(),
            U256::from(HOTKEY + 1),
            netuid,
            U256::from(COLDKEY)
        ));
        assert_ok!(SubtensorModule::spend_gamma(
            coldkey.clone(),
            netuid,
            spends(&[(HOTKEY + 1, GAMMA)])
        ));
        assert_ok!(SubtensorModule::close_gamma_deposit(
            coldkey,
            U256::from(HOTKEY + 1),
            netuid
        ));
        assert_ok!(SubtensorModule::do_swap_coldkey(
            &U256::from(COLDKEY),
            &U256::from(66)
        ));
        assert_eq!(deposit(netuid, HOTKEY).coldkey, U256::from(66));
        assert_gamma_invariants(netuid);
    });
}

#[test]
fn deposit_spender_survives_the_owners_coldkey_swap() {
    new_test_ext(1).execute_with(|| {
        let netuid = gamma_subnet();
        let (old_owner, new_owner) = (U256::from(OWNER_COLDKEY), U256::from(66));
        mint(netuid, COLDKEY, HOTKEY, TAO);
        assert_ok!(SubtensorModule::do_swap_coldkey(&old_owner, &new_owner));
        assert_eq!(SubtensorModule::gamma_default_spender(netuid), new_owner);
        assert_eq!(deposit(netuid, HOTKEY).spender, old_owner);
        let spend = |spender: U256| {
            let before = deposit(netuid, HOTKEY).balance;
            assert_ok!(SubtensorModule::spend_gamma(
                RuntimeOrigin::signed(spender),
                netuid,
                spends(&[(HOTKEY, 1)])
            ));
            before != deposit(netuid, HOTKEY).balance
        };
        assert!(!spend(new_owner));
        assert!(spend(old_owner));
    });
}

// =============================================================================
// Runtime API
// =============================================================================

#[test]
fn gamma_runtime_api_reads_deposits() {
    new_test_ext(1).execute_with(|| {
        let netuid = gamma_subnet();
        let other = add_dynamic_network(&U256::from(10), &U256::from(11));
        enable_gamma(other, 1_000 * TAO, 1_000 * TAO);
        let coldkey = U256::from(COLDKEY);
        mint(netuid, COLDKEY, HOTKEY, TAO);
        mint(other, COLDKEY, HOTKEY + 1, TAO);
        mint(other, COLDKEY + 100, HOTKEY + 2, TAO);
        assert_ok!(SubtensorModule::set_gamma_deposit_daily_spend_cap(
            RuntimeOrigin::signed(coldkey),
            U256::from(HOTKEY),
            netuid,
            Some(GAMMA.into()),
        ));

        let info = SubtensorModule::get_gamma_deposit(netuid, U256::from(HOTKEY)).unwrap();
        assert_eq!(info.netuid, netuid);
        assert_eq!(info.hotkey, U256::from(HOTKEY));
        assert_eq!(info.deposit, deposit(netuid, HOTKEY));
        assert_eq!(info.spendable, GAMMA.into());
        assert_eq!(
            SubtensorModule::get_gamma_deposit(netuid, U256::from(HOTKEY + 1)),
            None
        );

        let all = SubtensorModule::get_gamma_deposits_for_coldkey(&coldkey);
        assert_eq!(
            all.iter()
                .map(|info| (info.netuid, info.hotkey))
                .collect::<Vec<_>>(),
            vec![
                (netuid, U256::from(HOTKEY)),
                (other, U256::from(HOTKEY + 1))
            ]
        );
        assert!(SubtensorModule::get_gamma_deposits_for_coldkey(&U256::from(5)).is_empty());
    });
}

#[test]
fn sim_mint_gamma_equals_mint_gamma() {
    new_test_ext(1).execute_with(|| {
        let netuid = gamma_subnet();
        let coldkey = U256::from(COLDKEY);
        add_balance_to_coldkey_account(&coldkey, (12 * TAO).into());

        let root = sp_io::storage::root(sp_runtime::StateVersion::V1);
        let simulated = SubtensorModule::sim_mint_gamma(netuid, (10 * TAO).into()).unwrap();
        assert_eq!(sp_io::storage::root(sp_runtime::StateVersion::V1), root);
        assert_ok!(SubtensorModule::mint_gamma(
            RuntimeOrigin::signed(coldkey),
            U256::from(HOTKEY),
            netuid,
            (10 * TAO).into(),
            GammaBalance::ZERO,
        ));
        let (alpha, value, gamma) = simulated;
        System::assert_has_event(
            Event::GammaMinted {
                netuid,
                coldkey,
                hotkey: U256::from(HOTKEY),
                spender: U256::from(OWNER_COLDKEY),
                tao: (10 * TAO).into(),
                alpha,
                value,
                gamma,
            }
            .into(),
        );

        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(coldkey),
            U256::from(OWNER_HOTKEY),
            netuid,
            TAO.into()
        ));
        let stake = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &U256::from(OWNER_HOTKEY),
            &coldkey,
            netuid,
        );
        let simulated = SubtensorModule::sim_mint_gamma_with_stake(netuid, stake).unwrap();
        assert_ok!(SubtensorModule::mint_gamma_with_stake(
            RuntimeOrigin::signed(coldkey),
            U256::from(OWNER_HOTKEY),
            U256::from(HOTKEY),
            netuid,
            stake,
            GammaBalance::ZERO,
        ));
        let (alpha, value, gamma) = simulated;
        System::assert_has_event(
            Event::GammaMinted {
                netuid,
                coldkey,
                hotkey: U256::from(HOTKEY),
                spender: U256::from(OWNER_COLDKEY),
                tao: TaoBalance::ZERO,
                alpha,
                value,
                gamma,
            }
            .into(),
        );

        SubnetGammaEnabled::<Test>::insert(netuid, false);
        assert_eq!(SubtensorModule::sim_mint_gamma(netuid, TAO.into()), None);
        assert_eq!(
            SubtensorModule::sim_mint_gamma_with_stake(netuid, stake),
            None
        );
    });
}
