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
        assert!(
            staker_gain >= expected_staker.into() && staker_gain <= (expected_staker + 2).into(),
            "staker paid ~α×r: gain {staker_gain:?} expected {expected_staker}"
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
