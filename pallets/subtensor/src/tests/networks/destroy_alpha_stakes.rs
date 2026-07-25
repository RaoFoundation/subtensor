#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::unwrap_used)]
//! `destroy_alpha_in_out_stakes` pro-rata payouts, lock cleanup, and refund gating.

use super::helpers::*;
use super::prelude::*;

#[test]
fn destroy_alpha_out_multiple_stakers_pro_rata() {
    new_test_ext(0).execute_with(|| {
        // 1. Owner & subnet
        let owner_cold = U256::from(10);
        let owner_hot = U256::from(20);
        let netuid = add_dynamic_network(&owner_hot, &owner_cold);
        remove_owner_registration_stake(netuid);

        // Mark this subnet as *legacy* so owner refund path is enabled.
        let reg_at = NetworkRegisteredAt::<Test>::get(netuid);
        NetworkRegistrationStartBlock::<Test>::put(reg_at.saturating_add(1));

        // 2. Two stakers on that subnet
        let (c1, h1) = (U256::from(111), U256::from(211));
        let (c2, h2) = (U256::from(222), U256::from(333));
        register_ok_neuron(netuid, h1, c1, 0);
        register_ok_neuron(netuid, h2, c2, 0);

        // 3. Stake 30 : 70 (s1 : s2) in TAO
        let min_total = DefaultMinStake::<Test>::get();
        let min_total_u64: u64 = min_total.into();
        let s1: u64 = 3u64 * min_total_u64;
        let s2: u64 = 7u64 * min_total_u64;

        add_balance_to_coldkey_account(&c1, (s1 + 50_000).into());
        add_balance_to_coldkey_account(&c2, (s2 + 50_000).into());

        assert_ok!(SubtensorModule::do_add_stake(
            RuntimeOrigin::signed(c1),
            h1,
            netuid,
            s1.into()
        ));
        assert_ok!(SubtensorModule::do_add_stake(
            RuntimeOrigin::signed(c2),
            h2,
            netuid,
            s2.into()
        ));

        // 4. α-out snapshot

        SubnetAlphaIn::<Test>::insert(netuid, AlphaBalance::ZERO);
        SubnetProtocolAlpha::<Test>::insert(netuid, AlphaBalance::ZERO);
        let a1: u128 = sf_to_u128(&AlphaV2::<Test>::get((h1, c1, netuid)));
        let a2: u128 = sf_to_u128(&AlphaV2::<Test>::get((h2, c2, netuid)));
        let atotal = a1 + a2;

        // 5. TAO pot & lock
        let tao_pot: u64 = 10_000;
        SubnetTAO::<Test>::insert(netuid, TaoBalance::from(tao_pot));
        SubtensorModule::set_subnet_locked_balance(netuid, TaoBalance::from(5_000));

        // 6. Balances before
        let c1_before = SubtensorModule::get_coldkey_balance(&c1);
        let c2_before = SubtensorModule::get_coldkey_balance(&c2);
        let owner_before = SubtensorModule::get_coldkey_balance(&owner_cold);

        // 7. Run the (now credit-to-coldkey) logic
        destroy_alpha_in_out_stakes_full_pipeline_for_test(netuid);

        // 8. Expected τ shares via largest remainder
        let prod1 = (tao_pot as u128) * a1;
        let prod2 = (tao_pot as u128) * a2;
        let mut s1_share = (prod1 / atotal) as u64;
        let mut s2_share = (prod2 / atotal) as u64;
        let distributed = s1_share + s2_share;
        if distributed < tao_pot {
            // Assign leftover to larger remainder
            let r1 = prod1 % atotal;
            let r2 = prod2 % atotal;
            if r1 >= r2 {
                s1_share += 1;
            } else {
                s2_share += 1;
            }
        }

        // 9. Cold-key balances must have increased accordingly
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&c1),
            c1_before + s1_share.into()
        );
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&c2),
            c2_before + s2_share.into()
        );

        // 10. Owner refund (5 000 τ) to cold-key (no emission)
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&owner_cold),
            owner_before + 5_000.into()
        );

        // 11. α entries cleared for the subnet
        assert!(!AlphaV2::<Test>::contains_key((h1, c1, netuid)));
        assert!(!AlphaV2::<Test>::contains_key((h2, c2, netuid)));
    });
}

#[test]
fn destroy_alpha_in_out_stakes_cleans_locking_coldkeys() {
    new_test_ext(0).execute_with(|| {
        let owner_cold = U256::from(10);
        let owner_hot = U256::from(20);
        let netuid = add_dynamic_network(&owner_hot, &owner_cold);
        remove_owner_registration_stake(netuid);

        let coldkey = U256::from(111);
        let hotkey = U256::from(222);
        let other_netuid = NetUid::from(u16::from(netuid) + 1);
        let lock = LockState {
            locked_mass: 10u64.into(),
            conviction: U64F64::from_num(1),
            last_update: 1,
        };

        Lock::<Test>::insert((coldkey, netuid, hotkey), lock.clone());
        LockingColdkeys::<Test>::insert((netuid, hotkey, coldkey), ());
        Lock::<Test>::insert((coldkey, other_netuid, hotkey), lock);
        LockingColdkeys::<Test>::insert((other_netuid, hotkey, coldkey), ());

        DissolveCleanupQueue::<Test>::set(vec![netuid]);
        run_block_idle();

        assert!(!Lock::<Test>::contains_key((coldkey, netuid, hotkey)));
        assert!(!LockingColdkeys::<Test>::contains_key((
            netuid, hotkey, coldkey
        )));
        assert!(Lock::<Test>::contains_key((coldkey, other_netuid, hotkey)));
        assert!(LockingColdkeys::<Test>::contains_key((
            other_netuid,
            hotkey,
            coldkey
        )));
    });
}

#[test]
fn destroy_alpha_in_out_stakes_cleans_all_lock_aggregates() {
    new_test_ext(0).execute_with(|| {
        let owner_cold = U256::from(10);
        let owner_hot = U256::from(20);
        let netuid = add_dynamic_network(&owner_hot, &owner_cold);
        remove_owner_registration_stake(netuid);

        let coldkey = U256::from(111);
        let hotkey = U256::from(222);
        let other_netuid = NetUid::from(u16::from(netuid) + 1);
        let lock = LockState {
            locked_mass: 10u64.into(),
            conviction: U64F64::from_num(1),
            last_update: 1,
        };

        HotkeyLock::<Test>::insert(netuid, hotkey, lock.clone());
        DecayingHotkeyLock::<Test>::insert(netuid, hotkey, lock.clone());
        OwnerLock::<Test>::insert(netuid, lock.clone());
        DecayingOwnerLock::<Test>::insert(netuid, lock.clone());
        DecayingLock::<Test>::insert(coldkey, netuid, false);

        HotkeyLock::<Test>::insert(other_netuid, hotkey, lock.clone());
        DecayingHotkeyLock::<Test>::insert(other_netuid, hotkey, lock.clone());
        OwnerLock::<Test>::insert(other_netuid, lock.clone());
        DecayingOwnerLock::<Test>::insert(other_netuid, lock);
        DecayingLock::<Test>::insert(coldkey, other_netuid, false);

        DissolveCleanupQueue::<Test>::set(vec![netuid]);
        run_block_idle();

        assert!(!HotkeyLock::<Test>::contains_key(netuid, hotkey));
        assert!(!DecayingHotkeyLock::<Test>::contains_key(netuid, hotkey));
        assert!(!OwnerLock::<Test>::contains_key(netuid));
        assert!(!DecayingOwnerLock::<Test>::contains_key(netuid));
        assert!(!DecayingLock::<Test>::contains_key(coldkey, netuid));

        assert!(HotkeyLock::<Test>::contains_key(other_netuid, hotkey));
        assert!(DecayingHotkeyLock::<Test>::contains_key(
            other_netuid,
            hotkey
        ));
        assert!(OwnerLock::<Test>::contains_key(other_netuid));
        assert!(DecayingOwnerLock::<Test>::contains_key(other_netuid));
        assert!(DecayingLock::<Test>::contains_key(coldkey, other_netuid));
    });
}

#[allow(clippy::indexing_slicing)]
#[test]
fn destroy_alpha_out_many_stakers_complex_distribution() {
    new_test_ext(0).execute_with(|| {
        // ── 1) create subnet with 20 stakers ────────────────────────────────
        let owner_cold = U256::from(1_000);
        let owner_hot = U256::from(2_000);
        let netuid = add_dynamic_network(&owner_hot, &owner_cold);
        remove_owner_registration_stake(netuid);
        SubtensorModule::set_max_registrations_per_block(netuid, 1_000u16);
        SubtensorModule::set_target_registrations_per_interval(netuid, 1_000u16);

        // Mark this subnet as *legacy* so owner refund path is enabled.
        let reg_at = NetworkRegisteredAt::<Test>::get(netuid);
        NetworkRegistrationStartBlock::<Test>::put(reg_at.saturating_add(1));

        // Runtime-exact min amount = min_stake + fee
        let min_amount = {
            let min_stake = DefaultMinStake::<Test>::get();
            let fee = <Test as pallet::Config>::SwapInterface::approx_fee_amount(
                netuid.into(),
                min_stake,
            );
            // Double the fees because fee is calculated for min_stake, not for min_amount
            min_stake + fee * 2.into()
        };

        const N: usize = 20;
        let mut cold = [U256::zero(); N];
        let mut hot = [U256::zero(); N];
        let mut stake = [0u64; N];

        let min_amount_u64: u64 = min_amount.into();
        for i in 0..N {
            cold[i] = U256::from(10_000 + 2 * i as u32);
            hot[i] = U256::from(10_001 + 2 * i as u32);
            stake[i] = (i as u64 + 1u64) * min_amount_u64; // multiples of min_amount

            register_ok_neuron(netuid, hot[i], cold[i], 0);
            add_balance_to_coldkey_account(&cold[i], (stake[i] + 100_000).into());

            assert_ok!(SubtensorModule::do_add_stake(
                RuntimeOrigin::signed(cold[i]),
                hot[i],
                netuid,
                stake[i].into()
            ));
        }

        // ── 2) α-out snapshot ───────────────────────────────────────────────
        let mut alpha = [0u128; N];
        let mut alpha_sum: u128 = 0;
        for i in 0..N {
            alpha[i] = sf_to_u128(&AlphaV2::<Test>::get((hot[i], cold[i], netuid)));
            alpha_sum += alpha[i];
        }

        // ── 3) TAO pot & subnet lock ────────────────────────────────────────
        let tao_pot: u64 = 123_456;
        let lock: u64 = 30_000;
        SubnetTAO::<Test>::insert(netuid, TaoBalance::from(tao_pot));
        SubtensorModule::set_subnet_locked_balance(netuid, TaoBalance::from(lock));

        // ensure there was some Alpha issued
        assert!(SubtensorModule::get_alpha_issuance(netuid).to_u64() > 0);

        // Owner already earned some emission; owner-cut = 50 %
        SubnetOwnerCut::<Test>::put(32_768u16); // ~ 0.5 in fixed-point

        // ── 4) balances before ──────────────────────────────────────────────
        let mut bal_before = [TaoBalance::new(0); N];
        for i in 0..N {
            bal_before[i] = SubtensorModule::get_coldkey_balance(&cold[i]);
        }
        let owner_before = SubtensorModule::get_coldkey_balance(&owner_cold);

        // ── 5) expected τ share per pallet algorithm (incl. remainder) ─────

        SubnetAlphaIn::<Test>::insert(netuid, AlphaBalance::ZERO);
        SubnetProtocolAlpha::<Test>::insert(netuid, AlphaBalance::ZERO);
        let mut share = [0u64; N];
        let mut rem = [0u128; N];
        let mut paid: u128 = 0;

        for i in 0..N {
            let prod = tao_pot as u128 * alpha[i];
            share[i] = (prod / alpha_sum) as u64;
            rem[i] = prod % alpha_sum;
            paid += share[i] as u128;
        }
        let leftover = tao_pot as u128 - paid;
        let mut idx: Vec<_> = (0..N).collect();
        idx.sort_by_key(|i| core::cmp::Reverse(rem[*i]));
        for i in 0..leftover as usize {
            share[idx[i]] += 1;
        }

        // ── 5b) expected owner refund with price-aware emission deduction ───
        let frac: U96F32 = SubtensorModule::get_float_subnet_owner_cut();
        let total_emitted_alpha: u64 = SubtensorModule::get_alpha_issuance(netuid).to_u64();
        let owner_alpha_u64: u64 = U96F32::from_num(total_emitted_alpha)
            .saturating_mul(frac)
            .floor()
            .saturating_to_num::<u64>();

        let owner_emission_tao: u64 = {
            // Fallback matches the pallet's fallback
            let price: U96F32 = U96F32::from_num(
                <Test as pallet::Config>::SwapInterface::current_alpha_price(netuid.into()),
            );
            U96F32::from_num(owner_alpha_u64)
                .saturating_mul(price)
                .floor()
                .saturating_to_num::<u64>()
        };

        let expected_refund = lock.saturating_sub(owner_emission_tao);

        // ── 6) run distribution (credits τ to coldkeys, wipes α state) ─────
        destroy_alpha_in_out_stakes_full_pipeline_for_test(netuid);

        // ── 7) post checks ──────────────────────────────────────────────────
        for i in 0..N {
            // cold-key balances increased by expected τ share
            assert_eq!(
                SubtensorModule::get_coldkey_balance(&cold[i]),
                bal_before[i] + share[i].into(),
                "staker {i} cold-key balance changed unexpectedly"
            );
        }

        // owner refund
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&owner_cold),
            owner_before + expected_refund.into()
        );

        // α cleared for dissolved subnet & related counters reset
        assert!(AlphaV2::<Test>::iter().all(|((_h, _c, n), _)| n != netuid));
        assert_eq!(SubnetAlphaIn::<Test>::get(netuid), 0.into());
        assert_eq!(SubnetAlphaOut::<Test>::get(netuid), 0.into());
        assert_eq!(SubtensorModule::get_subnet_locked_balance(netuid), 0.into());
    });
}

#[test]
fn destroy_alpha_out_refund_gating_by_registration_block() {
    // ──────────────────────────────────────────────────────────────────────
    // Case A: LEGACY subnet → refund applied
    // ──────────────────────────────────────────────────────────────────────
    new_test_ext(0).execute_with(|| {
        // Owner + subnet
        let owner_cold = U256::from(10_000);
        let owner_hot = U256::from(20_000);
        let netuid = add_dynamic_network(&owner_hot, &owner_cold);
        remove_owner_registration_stake(netuid);

        // Mark as *legacy*: registered_at < start_block
        let reg_at = NetworkRegisteredAt::<Test>::get(netuid);
        NetworkRegistrationStartBlock::<Test>::put(reg_at.saturating_add(1));

        // Lock and (nonzero) emissions
        let lock_u64: u64 = 50_000;
        SubtensorModule::set_subnet_locked_balance(netuid, TaoBalance::from(lock_u64));
        // Owner cut ≈ 50%
        SubnetOwnerCut::<Test>::put(32_768u16);

        // give some stake to other key
        let other_cold = U256::from(1_234);
        let other_hot = U256::from(2_345);
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &other_hot,
            &other_cold,
            netuid,
            AlphaBalance::from(30u64), // not nearly enough to cover the lock
        );

        // ensure there was some Alpha issued
        assert!(SubtensorModule::get_alpha_issuance(netuid).to_u64() > 0);

        // Compute expected refund using the same math as the pallet
        let frac: U96F32 = SubtensorModule::get_float_subnet_owner_cut();
        let total_emitted_alpha: u64 = SubtensorModule::get_alpha_issuance(netuid).to_u64();
        let owner_alpha_u64: u64 = U96F32::from_num(total_emitted_alpha)
            .saturating_mul(frac)
            .floor()
            .saturating_to_num::<u64>();

        let owner_emission_tao_u64 = {
            let price: U96F32 = U96F32::from_num(
                <Test as pallet::Config>::SwapInterface::current_alpha_price(netuid.into()),
            );
            U96F32::from_num(owner_alpha_u64)
                .saturating_mul(price)
                .floor()
                .saturating_to_num::<u64>()
        };

        let expected_refund: u64 = lock_u64.saturating_sub(owner_emission_tao_u64);

        // Balances before
        let owner_before = SubtensorModule::get_coldkey_balance(&owner_cold);

        // Run the path under test
        let mut weight_meter =
            frame_support::weights::WeightMeter::with_limit(Weight::from_parts(u64::MAX, u64::MAX));
        // total alpha tracked in CurrentDissolveCleanupStatus;
        // distributed tao tracked in CurrentDissolveCleanupStatus;
        {
            let mut status = dissolve_cleanup_status(netuid);
            status.subnet_total_alpha_value = Some(0);
            SubtensorModule::destroy_alpha_in_out_stakes(netuid, &mut weight_meter, &mut status);
        }

        // Owner received their refund…
        let owner_after = SubtensorModule::get_coldkey_balance(&owner_cold);
        assert_eq!(owner_after, owner_before + expected_refund.into());

        // …and the lock is always cleared to zero by destroy_alpha_in_out_stakes.
        assert_eq!(
            SubtensorModule::get_subnet_locked_balance(netuid),
            TaoBalance::from(0u64)
        );
    });

    // ──────────────────────────────────────────────────────────────────────
    // Case B: NON‑LEGACY subnet → NO refund;
    // ──────────────────────────────────────────────────────────────────────
    new_test_ext(0).execute_with(|| {
        // Owner + subnet
        let owner_cold = U256::from(1_111);
        let owner_hot = U256::from(2_222);
        let netuid = add_dynamic_network(&owner_hot, &owner_cold);
        remove_owner_registration_stake(netuid);

        // Explicitly set start_block <= registered_at to make it non‑legacy.
        let reg_at = NetworkRegisteredAt::<Test>::get(netuid);
        NetworkRegistrationStartBlock::<Test>::put(reg_at);

        // Lock and emissions present (should be ignored for refund)
        let lock_u64: u64 = 42_000;
        SubtensorModule::set_subnet_locked_balance(netuid, TaoBalance::from(lock_u64));
        // give some stake to other key
        let other_cold = U256::from(1_234);
        let other_hot = U256::from(2_345);
        mock_increase_stake_for_hotkey_and_coldkey_on_subnet(
            &other_hot,
            &other_cold,
            netuid,
            AlphaBalance::from(300u64), // not nearly enough to cover the lock
        );
        // ensure there was some Alpha issued
        assert!(SubtensorModule::get_alpha_issuance(netuid).to_u64() > 0);
        SubnetOwnerCut::<Test>::put(32_768u16); // ~50%

        // Balances before
        let owner_before = SubtensorModule::get_coldkey_balance(&owner_cold);

        // Run the path under test
        let mut weight_meter =
            frame_support::weights::WeightMeter::with_limit(Weight::from_parts(u64::MAX, u64::MAX));
        // total alpha tracked in CurrentDissolveCleanupStatus;
        // distributed tao tracked in CurrentDissolveCleanupStatus;
        {
            let mut status = dissolve_cleanup_status(netuid);
            status.subnet_total_alpha_value = Some(0);
            SubtensorModule::destroy_alpha_in_out_stakes(netuid, &mut weight_meter, &mut status);
        }

        // No refund for non‑legacy
        let owner_after = SubtensorModule::get_coldkey_balance(&owner_cold);
        assert_eq!(owner_after, owner_before);

        // Lock is still cleared to zero by the routine
        assert_eq!(
            SubtensorModule::get_subnet_locked_balance(netuid),
            TaoBalance::from(0u64)
        );
    });

    // ──────────────────────────────────────────────────────────────────────
    // Case C: LEGACY subnet but lock = 0 → no refund;
    // ──────────────────────────────────────────────────────────────────────
    new_test_ext(0).execute_with(|| {
        // Owner + subnet
        let owner_cold = U256::from(9_999);
        let owner_hot = U256::from(8_888);
        let netuid = add_dynamic_network(&owner_hot, &owner_cold);
        remove_owner_registration_stake(netuid);

        // Mark as *legacy*
        let reg_at = NetworkRegisteredAt::<Test>::get(netuid);
        NetworkRegistrationStartBlock::<Test>::put(reg_at.saturating_add(1));

        // lock = 0; emissions present (must not matter)
        SubtensorModule::set_subnet_locked_balance(netuid, TaoBalance::from(0u64));
        SubnetAlphaOut::<Test>::insert(netuid, AlphaBalance::from(10_000));
        // ensure there was some Alpha issued
        assert!(SubtensorModule::get_alpha_issuance(netuid).to_u64() > 0);
        SubnetOwnerCut::<Test>::put(32_768u16); // ~50%

        let owner_before = SubtensorModule::get_coldkey_balance(&owner_cold);
        let mut weight_meter =
            frame_support::weights::WeightMeter::with_limit(Weight::from_parts(u64::MAX, u64::MAX));
        {
            let mut status = dissolve_cleanup_status(netuid);
            status.subnet_total_alpha_value = Some(0);
            SubtensorModule::destroy_alpha_in_out_stakes(netuid, &mut weight_meter, &mut status);
        }
        let owner_after = SubtensorModule::get_coldkey_balance(&owner_cold);

        // No refund possible when lock = 0
        assert_eq!(owner_after, owner_before);
        assert_eq!(
            SubtensorModule::get_subnet_locked_balance(netuid),
            TaoBalance::from(0u64)
        );
    });
}
