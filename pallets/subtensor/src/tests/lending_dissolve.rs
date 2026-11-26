#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing
)]

//! Dissolve with live synthetic shorts and frozen-`r` staker payout (SPEC P12).

use super::mock::*;
use crate::*;
use frame_support::assert_ok;
use frame_system::RawOrigin;
use pallet_lending::{self as lending, Pallet as LendingPallet};
use sp_core::U256;
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::{AlphaBalance, TaoBalance};

const SHORT_MINT: u64 = 100_000_000_000;

#[test]
fn dissolve_with_short_pays_stakers_at_frozen_r_and_recycles_leftover() {
    new_test_ext(0).execute_with(|| {
        let owner_cold = U256::from(10);
        let owner_hot = U256::from(11);
        let net = add_dynamic_network(&owner_hot, &owner_cold);
        remove_owner_registration_stake(net);
        let reserve_tao = TaoBalance::from(50_000_000_000_000u64);
        let reserve_alpha = AlphaBalance::from(50_000_000_000_000u64);
        setup_reserves(net, reserve_tao, reserve_alpha);

        // Cheap rank floor so `net` is outside the prune zone.
        let _other = add_dynamic_network(&U256::from(12), &U256::from(13));
        enable_lending_on_subnet(net);

        let short_cold = U256::from(301);
        let quote = LendingPallet::<Test>::quote_open_short(net, SHORT_MINT).unwrap();
        let stake_in = TaoBalance::from(quote.required_collateral)
            .saturating_add(TaoBalance::from(quote.deposit))
            .saturating_add(ExistentialDeposit::get());
        add_balance_to_coldkey_account(&short_cold, stake_in);
        assert_ok!(Lending::open_short(
            RawOrigin::Signed(short_cold).into(),
            net,
            SHORT_MINT,
            quote.required_collateral,
            quote.proceeds,
        ));

        let (s_hot, s_cold, stake) = (U256::from(401), U256::from(501), 500_000_000_000u64);
        AlphaV2::<Test>::insert((s_hot, s_cold, net), sf_from_u64(stake));
        TotalHotkeyAlpha::<Test>::insert(s_hot, net, AlphaBalance::from(stake));

        let pot: u64 = 8_000_000_000;
        SubnetTAO::<Test>::insert(net, TaoBalance::from(pot));
        SubtensorModule::set_subnet_locked_balance(net, 2_000.into());
        let owner_before = SubtensorModule::get_coldkey_balance(&owner_cold);
        let staker_before = SubtensorModule::get_coldkey_balance(&s_cold);

        let pot_before = SubnetTAO::<Test>::get(net).to_u64();

        assert_ok!(SubtensorModule::do_dissolve_network(net));
        let rate = lending::Dissolutions::<Test>::get(net).unwrap().price;
        run_dissolve_cleanup_to_completion();

        let expected_staker = (U64F64::saturating_from_num(stake).saturating_mul(rate))
            .floor()
            .saturating_to_num::<u64>();

        let staker_gain =
            SubtensorModule::get_coldkey_balance(&s_cold).saturating_sub(staker_before.into());
        // Adaptive payouts: the staker gets at most α × frozen_r, but the actual
        // amount may be less if the actual pot (subnet account balance) was
        // depleted relative to SubnetTAO (e.g., by the short sale draining the
        // pool account).  Verify the upper bound only.
        assert!(
            staker_gain > TaoBalance::ZERO,
            "staker received nothing: gain {staker_gain:?}"
        );
        assert!(
            staker_gain <= (expected_staker + 2).into(),
            "staker paid more than α×r: gain {staker_gain:?}, expected at most {expected_staker}"
        );

        let owner_gain =
            SubtensorModule::get_coldkey_balance(&owner_cold).saturating_sub(owner_before.into());
        assert!(
            owner_gain <= 2_000.into(),
            "leftover pot must not flow to owner as catch-all: owner gained {owner_gain:?}"
        );

        assert!(
            staker_gain < TaoBalance::from(pot_before),
            "staker did not absorb entire pot while shorts were open"
        );

        assert!(
            lending::SyntheticOutstanding::<Test>::get(net) == 0,
            "short book cleared"
        );
        assert!(lending::Dissolutions::<Test>::get(net).is_none());
        assert!(!SubnetTAO::<Test>::contains_key(net));
    });
}

/// HIGH 1C: Many stakers, no short — total payout bounded by the actual physical pot.
///
/// Sets `SubnetTAO` (bookkeeping) to 50T so `dissolution_rate ≈ 1.0`, then adds three
/// stakers each with 2T alpha.  Theoretical per-staker payout ≈ 2T TAO, but the subnet
/// account only holds the registration lock (100B), so the adaptive rate must cap all
/// three payouts.
#[test]
fn dissolve_basket_sale_then_many_stakers_payouts_bounded_by_pot() {
    new_test_ext(0).execute_with(|| {
        let owner_cold = U256::from(10);
        let owner_hot = U256::from(11);
        let net = add_dynamic_network(&owner_hot, &owner_cold);
        remove_owner_registration_stake(net);

        // Pool reserves: SubnetTAO = SubnetAlphaIn = 50T → dissolution_rate ≈ 1.0.
        let reserve_tao = TaoBalance::from(50_000_000_000_000u64);
        let reserve_alpha = AlphaBalance::from(50_000_000_000_000u64);
        setup_reserves(net, reserve_tao, reserve_alpha);

        // Second subnet keeps `net` outside the prune zone.
        let _other = add_dynamic_network(&U256::from(12), &U256::from(13));
        enable_lending_on_subnet(net);

        // Three stakers, each with 2T alpha inserted directly (SubnetAlphaOut stays 0,
        // so alpha_issuance = 50T, but total_alpha_value = 56T after counting stakes).
        let stake = 2_000_000_000_000u64; // 2T alpha
        let (s_hot1, s_cold1) = (U256::from(401), U256::from(501));
        let (s_hot2, s_cold2) = (U256::from(402), U256::from(502));
        let (s_hot3, s_cold3) = (U256::from(403), U256::from(503));
        for (hot, cold) in [(s_hot1, s_cold1), (s_hot2, s_cold2), (s_hot3, s_cold3)] {
            AlphaV2::<Test>::insert((hot, cold, net), sf_from_u64(stake));
            TotalHotkeyAlpha::<Test>::insert(hot, net, AlphaBalance::from(stake));
        }

        SubtensorModule::set_subnet_locked_balance(net, 2_000.into());

        // The subnet account physical balance (from the registration lock) is the
        // actual pot available for staker payouts — far less than what the frozen rate
        // would naively imply (3 × 2T × 1.0 = 6T >> 100B).
        let subnet_account = SubtensorModule::get_subnet_account_id(net)
            .expect("subnet account exists before dissolve");
        let initial_pot = SubtensorModule::get_coldkey_balance(&subnet_account);

        let staker1_before = SubtensorModule::get_coldkey_balance(&s_cold1);
        let staker2_before = SubtensorModule::get_coldkey_balance(&s_cold2);
        let staker3_before = SubtensorModule::get_coldkey_balance(&s_cold3);

        assert_ok!(SubtensorModule::do_dissolve_network(net));
        let rate = lending::Dissolutions::<Test>::get(net).unwrap().price;
        run_dissolve_cleanup_to_completion();

        // Upper bound: α × frozen_r (the rate frozen at basket completion).
        let expected_per_staker: u64 = (U64F64::saturating_from_num(stake).saturating_mul(rate))
            .floor()
            .saturating_to_num::<u64>();

        let gain1 =
            SubtensorModule::get_coldkey_balance(&s_cold1).saturating_sub(staker1_before.into());
        let gain2 =
            SubtensorModule::get_coldkey_balance(&s_cold2).saturating_sub(staker2_before.into());
        let gain3 =
            SubtensorModule::get_coldkey_balance(&s_cold3).saturating_sub(staker3_before.into());
        let total_payout = gain1.saturating_add(gain2).saturating_add(gain3);

        // Each staker paid at most α × frozen_r.
        assert!(
            gain1 <= (expected_per_staker + 2).into(),
            "staker1 overpaid: {gain1:?} vs {expected_per_staker}"
        );
        assert!(
            gain2 <= (expected_per_staker + 2).into(),
            "staker2 overpaid: {gain2:?} vs {expected_per_staker}"
        );
        assert!(
            gain3 <= (expected_per_staker + 2).into(),
            "staker3 overpaid: {gain3:?} vs {expected_per_staker}"
        );

        // Total payout cannot exceed the physical pot that was present at dissolution.
        assert!(
            total_payout <= initial_pot,
            "total payout {total_payout:?} exceeded initial pot {initial_pot:?}"
        );

        // The adaptive rate reduced each payout below the frozen-r expectation:
        // physical pot (100B lock) << expected_per_staker (2T × 1.0 = 2T).
        assert!(
            gain1 < expected_per_staker.into(),
            "staker1 should be capped by pot: got {gain1:?}, frozen_r implies {expected_per_staker}"
        );

        // All stakers received something (physical pot is nonzero).
        assert!(gain1 > TaoBalance::ZERO, "staker1 got nothing");
        assert!(gain2 > TaoBalance::ZERO, "staker2 got nothing");
        assert!(gain3 > TaoBalance::ZERO, "staker3 got nothing");

        // Lending state fully cleaned up.
        assert!(lending::Dissolutions::<Test>::get(net).is_none());
        assert!(!SubnetTAO::<Test>::contains_key(net));
    });
}
