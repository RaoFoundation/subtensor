#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::unwrap_used)]
//! `do_dissolve_network` refund, pro-rata TAO, and protocol-alpha share behavior.

use super::prelude::*;

#[test]
fn dissolve_no_stakers_no_alpha_no_emission() {
    new_test_ext(0).execute_with(|| {
        let cold = U256::from(1);
        let hot = U256::from(2);
        let net = add_dynamic_network(&hot, &cold);

        SubtensorModule::set_subnet_locked_balance(net, TaoBalance::from(0));
        SubnetTAO::<Test>::insert(net, TaoBalance::from(0));
        Emission::<Test>::insert(net, Vec::<AlphaBalance>::new());

        let before = SubtensorModule::get_coldkey_balance(&cold);
        assert_ok!(SubtensorModule::do_dissolve_network(net));
        let after = SubtensorModule::get_coldkey_balance(&cold);

        // Balance should be unchanged (whatever the network-lock bookkeeping left there)
        assert_eq!(after, before);
        assert!(!SubtensorModule::if_subnet_exist(net));
    });
}

#[test]
fn dissolve_refunds_full_lock_cost_when_no_emission() {
    new_test_ext(0).execute_with(|| {
        let cold = U256::from(3);
        let hot = U256::from(4);
        let net = add_dynamic_network(&hot, &cold);

        // Mark this subnet as *legacy* so owner refund path is enabled.
        let reg_at = NetworkRegisteredAt::<Test>::get(net);
        NetworkRegistrationStartBlock::<Test>::put(reg_at.saturating_add(1));

        let lock: TaoBalance = TaoBalance::from(1_000_000);
        SubtensorModule::set_subnet_locked_balance(net, lock);
        SubnetTAO::<Test>::insert(net, TaoBalance::from(0));
        Emission::<Test>::insert(net, Vec::<AlphaBalance>::new());

        let before = SubtensorModule::get_coldkey_balance(&cold);
        assert_ok!(SubtensorModule::do_dissolve_network(net));
        run_block_idle();
        let after = SubtensorModule::get_coldkey_balance(&cold);

        assert_eq!(TaoBalance::from(after), TaoBalance::from(before) + lock);
    });
}

#[test]
fn dissolve_single_alpha_out_staker_gets_all_tao() {
    new_test_ext(0).execute_with(|| {
        // 1. Owner & subnet
        let owner_cold = U256::from(10);
        let owner_hot = U256::from(20);
        let net = add_dynamic_network(&owner_hot, &owner_cold);
        remove_owner_registration_stake(net);
        SubnetAlphaIn::<Test>::insert(net, AlphaBalance::ZERO);
        SubnetProtocolAlpha::<Test>::insert(net, AlphaBalance::ZERO);

        // 2. Single α-out staker
        let (s_hot, s_cold) = (U256::from(100), U256::from(200));
        AlphaV2::<Test>::insert((s_hot, s_cold, net), sf_from_u64(5_000u64));

        // Entire TAO pot should be paid to staker's cold-key
        let pot: u64 = 99_999;
        SubnetTAO::<Test>::insert(net, TaoBalance::from(pot));
        SubtensorModule::set_subnet_locked_balance(net, 0.into());
        TotalHotkeyAlpha::<Test>::insert(s_hot, net, AlphaBalance::from(5_000u64));

        // Cold-key balance before
        let before = SubtensorModule::get_coldkey_balance(&s_cold);

        // Dissolve
        assert_ok!(SubtensorModule::do_dissolve_network(net));
        run_block_idle();

        // Cold-key received full pot
        let after = SubtensorModule::get_coldkey_balance(&s_cold);
        assert_eq!(after, before + pot.into());

        // No α entries left for dissolved subnet
        assert!(AlphaV2::<Test>::iter().all(|((_h, _c, n), _)| n != net));
        assert!(!SubnetTAO::<Test>::contains_key(net));
    });
}

#[allow(clippy::indexing_slicing)]
#[test]
fn dissolve_two_stakers_pro_rata_distribution() {
    new_test_ext(0).execute_with(|| {
        // Subnet + two stakers
        let oc = U256::from(50);
        let oh = U256::from(51);
        let net = add_dynamic_network(&oh, &oc);
        remove_owner_registration_stake(net);
        SubnetAlphaIn::<Test>::insert(net, AlphaBalance::ZERO);
        SubnetProtocolAlpha::<Test>::insert(net, AlphaBalance::ZERO);

        // Mark this subnet as *legacy* so owner refund path is enabled.
        let reg_at = NetworkRegisteredAt::<Test>::get(net);
        NetworkRegistrationStartBlock::<Test>::put(reg_at.saturating_add(1));

        let (s1_hot, s1_cold, a1) = (U256::from(201), U256::from(301), 300u64);
        let (s2_hot, s2_cold, a2) = (U256::from(202), U256::from(302), 700u64);

        AlphaV2::<Test>::insert((s1_hot, s1_cold, net), sf_from_u64(a1));
        AlphaV2::<Test>::insert((s2_hot, s2_cold, net), sf_from_u64(a2));

        TotalHotkeyAlpha::<Test>::insert(s1_hot, net, AlphaBalance::from(a1));
        TotalHotkeyAlpha::<Test>::insert(s2_hot, net, AlphaBalance::from(a2));

        let pot: u64 = 10_000;
        SubnetTAO::<Test>::insert(net, TaoBalance::from(pot));
        SubtensorModule::set_subnet_locked_balance(net, 5_000.into()); // owner refund path present; emission = 0

        // Cold-key balances before
        let s1_before = SubtensorModule::get_coldkey_balance(&s1_cold);
        let s2_before = SubtensorModule::get_coldkey_balance(&s2_cold);
        let owner_before = SubtensorModule::get_coldkey_balance(&oc);

        // Expected τ shares with largest remainder
        let total = (a1 + a2) as u128;
        let prod1 = (a1 as u128) * (pot as u128);
        let prod2 = (a2 as u128) * (pot as u128);
        let share1 = (prod1 / total) as u64;
        let share2 = (prod2 / total) as u64;
        let mut distributed = share1 + share2;
        let mut rem = [(s1_cold, prod1 % total), (s2_cold, prod2 % total)];
        if distributed < pot {
            rem.sort_by_key(|&(_c, r)| core::cmp::Reverse(r));
            let leftover = pot - distributed;
            for _ in 0..leftover as usize {
                distributed += 1;
            }
        }
        // Recompute exact expected shares using the same logic
        let mut expected1 = share1;
        let mut expected2 = share2;
        if share1 + share2 < pot {
            rem.sort_by_key(|&(_c, r)| core::cmp::Reverse(r));
            if rem[0].0 == s1_cold {
                expected1 += 1;
            } else {
                expected2 += 1;
            }
        }

        // Dissolve
        assert_ok!(SubtensorModule::do_dissolve_network(net));
        run_block_idle();

        // Cold-keys received their τ shares
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&s1_cold),
            s1_before + expected1.into()
        );
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&s2_cold),
            s2_before + expected2.into()
        );

        // Owner refunded lock (no emission)
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&oc),
            owner_before + 5_000.into()
        );

        // α entries for dissolved subnet gone
        assert!(AlphaV2::<Test>::iter().all(|((_h, _c, n), _)| n != net));
    });
}

#[test]
fn dissolve_owner_cut_refund_logic() {
    new_test_ext(0).execute_with(|| {
        let oc = U256::from(70);
        let oh = U256::from(71);
        let net = add_dynamic_network(&oh, &oc);
        remove_owner_registration_stake(net);

        // Mark this subnet as *legacy* so owner refund path is enabled.
        let reg_at = NetworkRegisteredAt::<Test>::get(net);
        NetworkRegistrationStartBlock::<Test>::put(reg_at.saturating_add(1));

        // One staker and a TAO pot (not relevant to refund amount).
        let sh = U256::from(77);
        let sc = U256::from(88);
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &sh,
            &sc,
            net,
            AlphaBalance::from(800u64),
        );
        SubnetTAO::<Test>::insert(net, TaoBalance::from(1_000));

        // Lock & emissions: total emitted α = 800.
        let lock: TaoBalance = TaoBalance::from(2_000);
        SubtensorModule::set_subnet_locked_balance(net, lock);
        // ensure there was some Alpha issued
        assert!(SubtensorModule::get_alpha_issuance(net).to_u64() > 0);

        // Owner cut = 11796 / 65535 (about 18%).
        SubnetOwnerCut::<Test>::put(11_796u16);

        // Compute expected refund with the SAME math as the pallet.
        let frac: U96F32 = SubtensorModule::get_float_subnet_owner_cut();
        let total_emitted_alpha: u64 = SubtensorModule::get_alpha_issuance(net).to_u64();
        let owner_alpha_u64: u64 = U96F32::from_num(total_emitted_alpha)
            .saturating_mul(frac)
            .floor()
            .saturating_to_num::<u64>();

        // Use the current alpha price to estimate the TAO equivalent.
        let owner_emission_tao = {
            let price: U96F32 = U96F32::saturating_from_num(
                <Test as pallet::Config>::SwapInterface::current_alpha_price(net.into()),
            );
            U96F32::from_num(owner_alpha_u64)
                .saturating_mul(price)
                .floor()
                .saturating_to_num::<u64>()
                .into()
        };

        let expected_refund: TaoBalance = lock.saturating_sub(owner_emission_tao);

        println!("expected_refund = {:?}", expected_refund);

        let before = SubtensorModule::get_coldkey_balance(&oc);
        assert_ok!(SubtensorModule::do_dissolve_network(net));
        run_block_idle();
        let after = SubtensorModule::get_coldkey_balance(&oc);

        assert!(after > before); // some refund is expected
        let gain: TaoBalance = after.saturating_sub(before.into());
        assert!(
            gain >= expected_refund,
            "owner should receive at least the lock-based refund: gain {gain:?} expected_refund {expected_refund:?}"
        );
    });
}

#[test]
fn dissolve_zero_refund_when_emission_exceeds_lock() {
    new_test_ext(0).execute_with(|| {
        let oc = U256::from(1_000);
        let oh = U256::from(2_000);
        let net = add_dynamic_network(&oh, &oc);
        remove_owner_registration_stake(net);

        SubtensorModule::set_subnet_locked_balance(net, TaoBalance::from(1_000));
        SubnetOwnerCut::<Test>::put(u16::MAX); // 100 %
        Emission::<Test>::insert(net, vec![AlphaBalance::from(2_000)]);

        let before = SubtensorModule::get_coldkey_balance(&oc);
        assert_ok!(SubtensorModule::do_dissolve_network(net));
        let after = SubtensorModule::get_coldkey_balance(&oc);

        assert_eq!(after, before); // no refund
    });
}

#[test]
fn dissolve_nonexistent_subnet_fails() {
    new_test_ext(0).execute_with(|| {
        assert_err!(
            SubtensorModule::do_dissolve_network(9_999.into()),
            Error::<Test>::SubnetNotExists
        );
    });
}

#[test]
fn dissolve_materializes_nonzero_protocol_reservoirs_before_cleanup() {
    new_test_ext(0).execute_with(|| {
        let owner_cold = U256::from(123);
        let owner_hot = U256::from(456);
        let net = add_dynamic_network(&owner_hot, &owner_cold);
        remove_owner_registration_stake(net);

        // Force the modern dissolve branch where pool alpha participates in
        // the protocol denominator.
        TaoInRefundDeploymentBlock::<Test>::put(0);
        NetworkRegisteredAt::<Test>::insert(net, 1);

        let reservoir_tao = TaoBalance::from(100_u64);
        let reservoir_alpha = AlphaBalance::from(100_u64);
        let staker_hot = U256::from(789);
        let staker_cold = U256::from(987);

        let subnet_account = SubtensorModule::get_subnet_account_id(net).unwrap();
        add_balance_to_coldkey_account(&subnet_account, reservoir_tao);

        SubnetTAO::<Test>::insert(net, TaoBalance::ZERO);
        SubtensorModule::set_subnet_locked_balance(net, TaoBalance::ZERO);
        SubnetAlphaIn::<Test>::insert(net, AlphaBalance::ZERO);
        SubnetProtocolAlpha::<Test>::insert(net, AlphaBalance::ZERO);
        AlphaV2::<Test>::insert((staker_hot, staker_cold, net), sf_from_u64(100u64));
        TotalHotkeyAlpha::<Test>::insert(staker_hot, net, AlphaBalance::from(100u64));
        pallet_subtensor_swap::BalancerTaoReservoir::<Test>::insert(net, reservoir_tao);
        pallet_subtensor_swap::BalancerAlphaReservoir::<Test>::insert(net, reservoir_alpha);

        let staker_before = SubtensorModule::get_coldkey_balance(&staker_cold);
        let issuance_before = TotalIssuance::<Test>::get();

        assert_ok!(SubtensorModule::do_dissolve_network(net));

        // do_dissolve_network only queues the destructive cleanup, but it must
        // materialize pending protocol reservoirs before the queued cleanup can
        // compute stake payouts.
        assert!(!pallet_subtensor_swap::BalancerTaoReservoir::<Test>::contains_key(net));
        assert!(!pallet_subtensor_swap::BalancerAlphaReservoir::<Test>::contains_key(net));
        assert_eq!(SubnetTAO::<Test>::get(net), reservoir_tao);
        assert_eq!(SubnetAlphaIn::<Test>::get(net), reservoir_alpha);
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&staker_cold),
            staker_before
        );

        run_block_idle();

        // Reservoir alpha is treated like materialized protocol pool alpha.
        // The staker owns half the denominator, so receives half the reservoir
        // TAO pot; the protocol share is recycled.
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&staker_cold),
            staker_before + TaoBalance::from(50_u64)
        );
        assert!(TotalIssuance::<Test>::get() < issuance_before);
        assert!(!NetworksAdded::<Test>::contains_key(net));
        assert!(!SubnetOwner::<Test>::contains_key(net));
        assert!(!SubnetAlphaIn::<Test>::contains_key(net));
        assert!(!SubnetProtocolAlpha::<Test>::contains_key(net));
        assert!(!pallet_subtensor_swap::BalancerTaoReservoir::<Test>::contains_key(net));
        assert!(!pallet_subtensor_swap::BalancerAlphaReservoir::<Test>::contains_key(net));
    });
}

#[test]
fn dissolve_alpha_out_but_zero_tao_no_rewards() {
    new_test_ext(0).execute_with(|| {
        let oc = U256::from(21);
        let oh = U256::from(22);
        let net = add_dynamic_network(&oh, &oc);

        let sh = U256::from(23);
        let sc = U256::from(24);

        AlphaV2::<Test>::insert((sh, sc, net), sf_from_u64(1_000u64));
        SubnetTAO::<Test>::insert(net, TaoBalance::from(0)); // zero TAO
        SubtensorModule::set_subnet_locked_balance(net, TaoBalance::from(0));
        Emission::<Test>::insert(net, Vec::<AlphaBalance>::new());
        TotalHotkeyAlpha::<Test>::insert(sh, net, AlphaBalance::from(1_000u64));

        let before = SubtensorModule::get_coldkey_balance(&sc);
        assert_ok!(SubtensorModule::do_dissolve_network(net));
        run_block_idle();
        let after = SubtensorModule::get_coldkey_balance(&sc);

        // No reward distributed, α-out cleared.
        assert_eq!(after, before);
        assert!(AlphaV2::<Test>::iter().next().is_none());
    });
}

#[test]
fn dissolve_decrements_total_networks() {
    new_test_ext(0).execute_with(|| {
        let total_before = TotalNetworks::<Test>::get();

        let cold = U256::from(41);
        let hot = U256::from(42);
        let net = add_dynamic_network(&hot, &cold);

        // Add 100 TAO to subnet account (lock)
        let subnet_account = SubtensorModule::get_subnet_account_id(net).unwrap();
        add_balance_to_coldkey_account(&subnet_account, 100_000_000_000_u64.into());

        // Sanity: adding network increments the counter.
        assert_eq!(TotalNetworks::<Test>::get(), total_before + 1);

        assert_ok!(SubtensorModule::do_dissolve_network(net));
        assert_eq!(TotalNetworks::<Test>::get(), total_before);
    });
}

#[test]
fn dissolve_rounding_remainder_distribution() {
    new_test_ext(0).execute_with(|| {
        // 1. Build subnet with two α-out stakers (3 & 2 α)
        let oc = U256::from(61);
        let oh = U256::from(62);
        let net = add_dynamic_network(&oh, &oc);
        remove_owner_registration_stake(net);
        SubnetAlphaIn::<Test>::insert(net, AlphaBalance::ZERO);
        SubnetProtocolAlpha::<Test>::insert(net, AlphaBalance::ZERO);

        let (s1h, s1c) = (U256::from(63), U256::from(64));
        let (s2h, s2c) = (U256::from(65), U256::from(66));

        AlphaV2::<Test>::insert((s1h, s1c, net), sf_from_u64(3u64));
        AlphaV2::<Test>::insert((s2h, s2c, net), sf_from_u64(2u64));

        SubnetTAO::<Test>::insert(net, TaoBalance::from(1)); // TAO pot = 1
        SubtensorModule::set_subnet_locked_balance(net, TaoBalance::from(0));

        TotalHotkeyAlpha::<Test>::insert(s1h, net, AlphaBalance::from(3u64));
        TotalHotkeyAlpha::<Test>::insert(s2h, net, AlphaBalance::from(2u64));

        // Cold-key balances before
        let c1_before = SubtensorModule::get_coldkey_balance(&s1c);
        let c2_before = SubtensorModule::get_coldkey_balance(&s2c);

        // 3. Run full dissolve flow
        assert_ok!(SubtensorModule::do_dissolve_network(net));
        run_block_idle();

        // 4. s1 (larger remainder) should get +1 τ on cold-key
        let c1_after = SubtensorModule::get_coldkey_balance(&s1c);
        let c2_after = SubtensorModule::get_coldkey_balance(&s2c);

        assert_eq!(c1_after, c1_before + 1.into());
        assert_eq!(c2_after, c2_before);

        // α records for subnet gone; TAO key gone
        assert!(AlphaV2::<Test>::iter().all(|((_h, _c, n), _)| n != net));
        assert!(!SubnetTAO::<Test>::contains_key(net));
    });
}

#[test]
fn dissolve_protocol_alpha_share_is_not_paid_to_users() {
    new_test_ext(0).execute_with(|| {
        let owner_cold = U256::from(610);
        let owner_hot = U256::from(620);
        let net = add_dynamic_network(&owner_hot, &owner_cold);
        remove_owner_registration_stake(net);

        // Make this subnet pre-deploy for protocol-alpha accounting.
        let reg_at = NetworkRegisteredAt::<Test>::get(net);
        TaoInRefundDeploymentBlock::<Test>::put(reg_at.saturating_add(1));
        SubtensorModule::set_subnet_locked_balance(net, TaoBalance::ZERO);

        // Alpha-in is the AMM pool reserve and must NOT participate in the
        // deregistration settlement for pre-deploy subnets. Only the chain-bought
        // cached protocol
        // alpha is converted to TAO pro-rata, exactly like every staker's alpha.
        SubnetAlphaIn::<Test>::insert(net, AlphaBalance::from(100u64));
        SubnetProtocolAlpha::<Test>::insert(net, AlphaBalance::from(50u64));

        let staker_hot = U256::from(630);
        let staker_cold = U256::from(640);
        AlphaV2::<Test>::insert((staker_hot, staker_cold, net), sf_from_u64(50u64));
        TotalHotkeyAlpha::<Test>::insert(staker_hot, net, AlphaBalance::from(50u64));

        let pot: u64 = 200;
        SubnetTAO::<Test>::insert(net, TaoBalance::from(pot));

        let staker_before = SubtensorModule::get_coldkey_balance(&staker_cold);
        let owner_before = SubtensorModule::get_coldkey_balance(&owner_cold);

        assert_ok!(SubtensorModule::do_dissolve_network(net));

        run_block_idle();

        // User gets 50 / (100 alpha-in + 50 cached protocol alpha + 50 user alpha)
        // of the TAO pot. The protocol share is withheld from user/owner payout.
        // Settlement denominator = 50 cached protocol alpha + 50 user alpha = 100
        // (alpha-in is excluded). The user therefore gets 50/100 of the 200 TAO pot,
        // i.e. 100 TAO. The chain-bought alpha's 100 TAO share is withheld from the
        // user/owner payout (it is recycled back to the chain, see the dedicated
        // recycling test below).
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&staker_cold),
            staker_before + 100.into()
        );
        // The owner is not paid the protocol share either (locked balance is zero, so
        // there is no refund path that could leak it).
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&owner_cold),
            owner_before
        );
        assert!(!SubnetProtocolAlpha::<Test>::contains_key(net));
    });
}

#[test]
fn dissolve_protocol_alpha_post_deploy_includes_alpha_in() {
    new_test_ext(0).execute_with(|| {
        let owner_cold = U256::from(611);
        let owner_hot = U256::from(621);

        let net = add_dynamic_network(&owner_hot, &owner_cold);
        remove_owner_registration_stake(net);

        // Make this subnet post-deploy for protocol-alpha accounting.
        TaoInRefundDeploymentBlock::<Test>::put(100);
        NetworkRegisteredAt::<Test>::insert(net, 101);

        SubtensorModule::set_subnet_locked_balance(net, TaoBalance::ZERO);

        SubnetAlphaIn::<Test>::insert(net, AlphaBalance::from(100u64));
        SubnetProtocolAlpha::<Test>::insert(net, AlphaBalance::from(50u64));

        let staker_hot = U256::from(631);
        let staker_cold = U256::from(641);

        AlphaV2::<Test>::insert((staker_hot, staker_cold, net), sf_from_u64(50u64));
        TotalHotkeyAlpha::<Test>::insert(staker_hot, net, AlphaBalance::from(50u64));

        let pot: u64 = 200;
        SubnetTAO::<Test>::insert(net, TaoBalance::from(pot));

        let staker_before = SubtensorModule::get_coldkey_balance(&staker_cold);
        let owner_before = SubtensorModule::get_coldkey_balance(&owner_cold);

        assert_ok!(SubtensorModule::do_dissolve_network(net));
        run_block_idle();

        // Post-deploy denominator = 100 alpha-in + 50 cached protocol alpha
        // + 50 user alpha = 200. The user gets 50/200 of the 200 TAO pot.
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&staker_cold),
            staker_before + 50.into()
        );

        assert_eq!(
            SubtensorModule::get_coldkey_balance(&owner_cold),
            owner_before
        );

        assert!(!SubnetProtocolAlpha::<Test>::contains_key(net));
    });
}

#[test]
fn dissolve_chain_bought_alpha_is_converted_to_tao_and_recycled() {
    new_test_ext(0).execute_with(|| {
        let owner_cold = U256::from(710);
        let owner_hot = U256::from(720);
        let net = add_dynamic_network(&owner_hot, &owner_cold);
        remove_owner_registration_stake(net);

        // Make this subnet pre-deploy for protocol-alpha accounting.
        let reg_at = NetworkRegisteredAt::<Test>::get(net);
        TaoInRefundDeploymentBlock::<Test>::put(reg_at.saturating_add(1));
        // No owner refund path: any TAO left on the subnet account is recycled.
        SubtensorModule::set_subnet_locked_balance(net, TaoBalance::ZERO);

        // Alpha-in is present but ignored on the pre-deploy branch. The cached
        // protocol alpha is the only claimant, so the entire pot is recycled.
        SubnetAlphaIn::<Test>::insert(net, AlphaBalance::from(123u64));
        SubnetProtocolAlpha::<Test>::insert(net, AlphaBalance::from(100u64));

        let pot: u64 = 100;
        SubnetTAO::<Test>::insert(net, TaoBalance::from(pot));

        let issuance_before = TotalIssuance::<Test>::get();
        let owner_before = SubtensorModule::get_coldkey_balance(&owner_cold);

        assert_ok!(SubtensorModule::do_dissolve_network(net));
        run_block_idle();

        // There are no stakers, so the entire pot is the chain-bought alpha's TAO
        // share. It is not paid to the owner; instead it is recycled back to the
        // chain, which removes it from existence and reduces total issuance.
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&owner_cold),
            owner_before
        );
        assert!(
            TotalIssuance::<Test>::get() < issuance_before,
            "recycling the chain-bought alpha's TAO must reduce total issuance"
        );
        assert!(!SubnetProtocolAlpha::<Test>::contains_key(net));
    });
}
