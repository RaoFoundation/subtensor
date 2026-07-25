#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::unwrap_used)]
//! End-to-end dissolve refund + re-registration lossless flow.

use super::prelude::*;

#[test]
fn massive_dissolve_refund_and_reregistration_flow_is_lossless_and_cleans_state() {
    new_test_ext(0).execute_with(|| {
        // ────────────────────────────────────────────────────────────────────
        // 0) Constants and helpers (distinct hotkeys & coldkeys)
        // ────────────────────────────────────────────────────────────────────
        const NUM_NETS: usize = 4;

        // Six LP coldkeys
        let cold_lps: [U256; 6] = [
            U256::from(3001),
            U256::from(3002),
            U256::from(3003),
            U256::from(3004),
            U256::from(3005),
            U256::from(3006),
        ];

        // For each coldkey, define two DISTINCT hotkeys it owns.
        let mut cold_to_hots: BTreeMap<U256, [U256; 2]> = BTreeMap::new();
        for &c in cold_lps.iter() {
            let h1 = U256::from(c.low_u64().saturating_add(100_000));
            let h2 = U256::from(c.low_u64().saturating_add(200_000));
            cold_to_hots.insert(c, [h1, h2]);
        }

        // Distinct τ pot sizes per net.
        let pots: [u64; NUM_NETS] = [12_345, 23_456, 34_567, 45_678];

        let lp_sets_per_net: [&[U256]; NUM_NETS] = [
            &cold_lps[0..4], // net0: A,B,C,D
            &cold_lps[2..6], // net1: C,D,E,F
            &cold_lps[0..6], // net2: A..F
            &cold_lps[1..5], // net3: B,C,D,E
        ];

        // ────────────────────────────────────────────────────────────────────
        // 1) Create many subnets, fix price at tick=0
        // ────────────────────────────────────────────────────────────────────
        let mut nets: Vec<NetUid> = Vec::new();
        for i in 0..NUM_NETS {
            let owner_hot = U256::from(10_000 + (i as u64));
            let owner_cold = U256::from(20_000 + (i as u64));
            let net = add_dynamic_network(&owner_hot, &owner_cold);
        remove_owner_registration_stake(net);
            SubtensorModule::set_max_registrations_per_block(net, 1_000u16);
            SubtensorModule::set_target_registrations_per_interval(net, 1_000u16);
            Emission::<Test>::insert(net, Vec::<AlphaBalance>::new());
            SubtensorModule::set_subnet_locked_balance(net, TaoBalance::from(0));

            nets.push(net);
        }

        // Map net → index for quick lookups.
        let mut net_index: BTreeMap<NetUid, usize> = BTreeMap::new();
        for (i, &n) in nets.iter().enumerate() {
            net_index.insert(n, i);
        }

        // ────────────────────────────────────────────────────────────────────
        // 2) Pre-create a handful of small (hot, cold) pairs so accounts exist
        // ────────────────────────────────────────────────────────────────────
        for id in 0u64..10 {
            let cold_acc = U256::from(1_000_000 + id);
            let hot_acc = U256::from(2_000_000 + id);
            for &net in nets.iter() {
                register_ok_neuron(net, hot_acc, cold_acc, 100_000 + id);
            }
        }

        // ────────────────────────────────────────────────────────────────────
        // 3) LPs per net: register each (hot, cold), massive τ prefund, and stake
        // ────────────────────────────────────────────────────────────────────
        for &cold in cold_lps.iter() {
            add_balance_to_coldkey_account(&cold, 1_000_000_000_000_u64.into());
        }

        // τ balances before LP adds (after staking):
        let mut tao_before: BTreeMap<U256, TaoBalance> = BTreeMap::new();

        // Ordered α snapshot per net at **pair granularity** (pre‑LP):
        let mut alpha_pairs_per_net: BTreeMap<NetUid, Vec<((U256, U256), u128)>> = BTreeMap::new();

        // Register both hotkeys for each participating cold on each net and stake τ→α.
        for (ni, &net) in nets.iter().enumerate() {
            let participants = lp_sets_per_net[ni];
            for &cold in participants.iter() {
                let [hot1, hot2] = cold_to_hots[&cold];

                // Ensure (hot, cold) neurons exist on this net.
                register_ok_neuron(
                    net,
                    hot1,
                    cold,
                    (ni as u64) * 10_000 + (hot1.low_u64() % 10_000),
                );
                register_ok_neuron(
                    net,
                    hot2,
                    cold,
                    (ni as u64) * 10_000 + (hot2.low_u64() % 10_000) + 1,
                );

                // Stake τ (split across the two hotkeys).
                let base: u64 =
                    5_000_000 + ((ni as u64) * 1_000_000) + ((cold.low_u64() % 10) * 250_000);
                let stake1: u64 = base.saturating_mul(3) / 5; // 60%
                let stake2: u64 = base.saturating_sub(stake1); // 40%

                assert_ok!(SubtensorModule::do_add_stake(
                    RuntimeOrigin::signed(cold),
                    hot1,
                    net,
                    stake1.into()
                ));
                assert_ok!(SubtensorModule::do_add_stake(
                    RuntimeOrigin::signed(cold),
                    hot2,
                    net,
                    stake2.into()
                ));
            }
        }

        // Record τ balances now (post‑stake, pre‑LP).
        for &cold in cold_lps.iter() {
            tao_before.insert(cold, SubtensorModule::get_coldkey_balance(&cold).into());
        }

        // Capture **pair‑level** α snapshot per net (pre‑LP).
        for ((hot, cold, net), amt) in AlphaV2::<Test>::iter() {
            if let Some(&ni) = net_index.get(&net)
                && lp_sets_per_net[ni].contains(&cold) {
                    let a: u128 = sf_to_u128(&amt);
                    if a > 0 {
                        alpha_pairs_per_net
                            .entry(net)
                            .or_default()
                            .push(((hot, cold), a));
                    }
                }
        }

        // Snapshot τ balances AFTER LP adds (to measure actual principal debit).
        let mut tao_after_adds: BTreeMap<U256, TaoBalance> = BTreeMap::new();
        for &cold in cold_lps.iter() {
            tao_after_adds.insert(cold, SubtensorModule::get_coldkey_balance(&cold));
        }

        // ────────────────────────────────────────────────────────────────────
        // 5) Compute Hamilton-apportionment BASE shares per cold and total leftover
        //    from the **pair-level** pre‑LP α snapshot; also count pairs per cold.
        // ────────────────────────────────────────────────────────────────────
        for &net in nets.iter() {
            SubnetAlphaIn::<Test>::insert(net, AlphaBalance::ZERO);
            SubnetProtocolAlpha::<Test>::insert(net, AlphaBalance::ZERO);
        }

        let mut base_share_cold: BTreeMap<U256, u64> =
            cold_lps.iter().copied().map(|c| (c, 0_u64)).collect();
        let mut pair_count_cold: BTreeMap<U256, u32> =
            cold_lps.iter().copied().map(|c| (c, 0_u32)).collect();

        let mut leftover_total: u64 = 0;

        for (ni, &net) in nets.iter().enumerate() {
            let pot = pots[ni];
            let pairs = alpha_pairs_per_net.get(&net).cloned().unwrap_or_default();
            if pot == 0 || pairs.is_empty() {
                continue;
            }
            let total_alpha: u128 = pairs.iter().map(|(_, a)| *a).sum();
            if total_alpha == 0 {
                continue;
            }

            let mut base_sum_net: u64 = 0;
            for ((_, cold), a) in pairs.iter().copied() {
                // quota = a * pot / total_alpha
                let prod: u128 = a.saturating_mul(pot as u128);
                let base: u64 = (prod / total_alpha) as u64;
                base_sum_net = base_sum_net.saturating_add(base);
                *base_share_cold.entry(cold).or_default() =
                    base_share_cold[&cold].saturating_add(base);
                *pair_count_cold.entry(cold).or_default() += 1;
            }
            let leftover_net = pot.saturating_sub(base_sum_net);
            leftover_total = leftover_total.saturating_add(leftover_net);
        }

        // ────────────────────────────────────────────────────────────────────
        // 6) Seed τ pots and dissolve *all* networks (liquidates LPs + refunds)
        // ────────────────────────────────────────────────────────────────────
        for (ni, &net) in nets.iter().enumerate() {
            SubnetTAO::<Test>::insert(net, TaoBalance::from(pots[ni]));
        }
        for &net in nets.iter() {
            assert_ok!(SubtensorModule::do_dissolve_network(net));
            run_block_idle();
        }

        // ────────────────────────────────────────────────────────────────────
        // 7) Assertions: τ balances, α gone, nets removed, swap state clean
        //    (Hamilton invariants enforced at cold-level without relying on tie-break)
        // ────────────────────────────────────────────────────────────────────
        // Collect actual pot credits per cold (principal cancels out against adds when comparing before→after).
        let mut actual_pot_cold: BTreeMap<U256, u64> =
            cold_lps.iter().copied().map(|c| (c, 0_u64)).collect();
        for &cold in cold_lps.iter() {
            let before = tao_before[&cold];
            let after = SubtensorModule::get_coldkey_balance(&cold);
            actual_pot_cold.insert(cold, after.saturating_sub(before.into()).into());
        }

        // (a) Sum of actual pot credits equals total pots.
        let total_actual: u64 = actual_pot_cold.values().copied().sum();
        let total_pots: u64 = pots.iter().copied().sum();
        assert_eq!(
            total_actual, total_pots,
            "total τ pot credited across colds must equal sum of pots"
        );

        // (b) Each cold’s pot is within Hamilton bounds: base ≤ actual ≤ base + #pairs.
        let mut extra_accum: u64 = 0;
        for &cold in cold_lps.iter() {
            let base = *base_share_cold.get(&cold).unwrap_or(&0);
            let pairs = *pair_count_cold.get(&cold).unwrap_or(&0) as u64;
            let actual = *actual_pot_cold.get(&cold).unwrap_or(&0);

            assert!(
                actual >= base,
                "cold {cold:?} actual pot {actual} is below base {base}"
            );
            assert!(
                actual <= base.saturating_add(pairs),
                "cold {cold:?} actual pot {actual} exceeds base + pairs ({base} + {pairs})"
            );

            extra_accum = extra_accum.saturating_add(actual.saturating_sub(base));
        }

        // (c) The total “extra beyond base” equals the computed leftover_total across nets.
        assert_eq!(
            extra_accum, leftover_total,
            "sum of extras beyond base must equal total leftover"
        );

        // (d) τ principal was fully refunded (compare after_adds → after).
        for &cold in cold_lps.iter() {
            let before = tao_before[&cold];
            let mid = tao_after_adds[&cold];
            let after = SubtensorModule::get_coldkey_balance(&cold);
            let principal_actual = before.saturating_sub(mid);
            let actual_pot = after.saturating_sub(before.into());
            assert_eq!(
                after.saturating_sub(mid.into()),
                principal_actual.saturating_add(actual_pot.into()).into(),
                "cold {cold:?} τ balance incorrect vs 'after_adds'"
            );
        }

        // For each dissolved net, check α ledgers gone, network removed, and swap state clean.
        for &net in nets.iter() {
            assert!(
                AlphaV2::<Test>::iter().all(|((_h, _c, n), _)| n != net),
                "alpha ledger not fully cleared for net {net:?}"
            );
            assert!(
                !SubtensorModule::if_subnet_exist(net),
                "subnet {net:?} still exists"
            );
            assert!(
                !pallet_subtensor_swap::PalSwapInitialized::<Test>::get(net),
                "PalSwapInitialized still set"
            );
        }

        // ────────────────────────────────────────────────────────────────────
        // 8) Re-register a fresh subnet and re‑stake using the pallet’s min rule
        //    Assert αΔ equals the sim-swap result for the exact τ staked.
        // ────────────────────────────────────────────────────────────────────
        let new_owner_hot = U256::from(99_000);
        let new_owner_cold = U256::from(99_001);
        let net_new = add_dynamic_network(&new_owner_hot, &new_owner_cold);
        remove_owner_registration_stake(net_new);
        SubtensorModule::set_max_registrations_per_block(net_new, 1_000u16);
        SubtensorModule::set_target_registrations_per_interval(net_new, 1_000u16);
        Emission::<Test>::insert(net_new, Vec::<AlphaBalance>::new());
        SubtensorModule::set_subnet_locked_balance(net_new, TaoBalance::from(0));

        // Compute the exact min stake per the pallet rule: DefaultMinStake + fee(DefaultMinStake).
        let min_stake = DefaultMinStake::<Test>::get();
		let order = GetAlphaForTao::<Test>::with_amount(min_stake);
        let fee_for_min = pallet_subtensor_swap::Pallet::<Test>::sim_swap(
            net_new,
			order,
        )
        .map(|r| r.fee_paid)
        .unwrap_or_else(|_e| {
            <pallet_subtensor_swap::Pallet<Test> as subtensor_swap_interface::SwapHandler>::approx_fee_amount(net_new, min_stake)
        });
        let min_amount_required = min_stake.saturating_add(fee_for_min).to_u64();

        // Re‑stake from three coldkeys; choose a specific DISTINCT hotkey per cold.
        for &cold in &cold_lps[0..3] {
            let [hot1, _hot2] = cold_to_hots[&cold];
            register_ok_neuron(net_new, hot1, cold, 7777);

            let before_tao = SubtensorModule::get_coldkey_balance(&cold);
            let a_prev: u64 = sf_to_u128(&AlphaV2::<Test>::get((hot1, cold, net_new))) as u64;

            // Expected α for this exact τ, using the same sim path as the pallet.
			let order = GetAlphaForTao::<Test>::with_amount(min_amount_required);
            let expected_alpha_out = pallet_subtensor_swap::Pallet::<Test>::sim_swap(
                net_new,
				order,
            )
            .map(|r| r.amount_paid_out)
            .expect("sim_swap must succeed for fresh net and min amount");

            assert_ok!(SubtensorModule::do_add_stake(
                RuntimeOrigin::signed(cold),
                hot1,
                net_new,
                min_amount_required.into()
            ));

            let after_tao = SubtensorModule::get_coldkey_balance(&cold);
            let a_new: u64 = sf_to_u128(&AlphaV2::<Test>::get((hot1, cold, net_new))) as u64;
            let a_delta = a_new.saturating_sub(a_prev);

            // τ decreased by exactly the amount we sent.
            assert_eq!(
                after_tao,
                before_tao.saturating_sub(min_amount_required.into()),
                "τ did not decrease by the min required restake amount for cold {cold:?}"
            );

            // α minted equals the simulated swap’s net out for that same τ.
            assert_eq!(
                a_delta, expected_alpha_out.to_u64(),
                "α minted mismatch for cold {cold:?} (hot {hot1:?}) on new net (αΔ {a_delta}, expected {expected_alpha_out})"
            );
        }
    });
}
