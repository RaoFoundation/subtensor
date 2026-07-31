#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::unwrap_used)]

use super::mock::*;
use crate::migrations::migrate_network_immunity_period;
use crate::staking::lock::LockState;
use crate::subnets::subnet::RefusalWindow;
use crate::*;
use frame_support::{
    assert_err, assert_noop, assert_ok, traits::Currency, weights::Weight, weights::WeightMeter,
};
use frame_system::Config;
use sp_core::U256;
use sp_runtime::PerU16;
use sp_std::collections::{btree_map::BTreeMap, vec_deque::VecDeque};
use substrate_fixed::types::{I96F32, U64F64, U96F32};
use subtensor_runtime_common::{MechId, NetUidStorageIndex, TaoBalance};
use subtensor_swap_interface::{Order, SwapHandler};

/// Run the same α-out destroy steps as `remove_data_for_dissolved_networks` (post-root-cleanup).
fn destroy_alpha_in_out_stakes_full_pipeline_for_test(netuid: NetUid) {
    run_destroy_alpha_in_out_stakes_full_pipeline(netuid);
}

#[test]
fn test_registration_ok() {
    new_test_ext(1).execute_with(|| {
        let block_number: u64 = 0;
        let netuid = NetUid::from(2);
        let tempo: u16 = 13;
        let hotkey_account_id: U256 = U256::from(1);
        let coldkey_account_id: U256 = U256::from(0); // Neighbour of the beast, har har

        add_network(netuid, tempo, 0);

        // Ensure reserves exist for any registration path that might touch swap/burn logic.
        let reserve: u64 = 1_000_000_000_000;
        setup_reserves(
            netuid,
            TaoBalance::from(reserve),
            AlphaBalance::from(reserve),
        );

        // registration economics changed. Ensure the coldkey has enough spendable balance
        add_balance_to_coldkey_account(&coldkey_account_id, TaoBalance::from(reserve));
        add_balance_to_coldkey_account(&hotkey_account_id, TaoBalance::from(reserve));

        let (nonce, work): (u64, Vec<u8>) = SubtensorModule::create_work_for_block_number(
            netuid,
            block_number,
            129123813,
            &hotkey_account_id,
        );

        // PoW register should succeed.
        assert_ok!(SubtensorModule::register(
            <<Test as Config>::RuntimeOrigin>::signed(hotkey_account_id),
            netuid,
            block_number,
            nonce,
            work.clone(),
            hotkey_account_id,
            coldkey_account_id
        ));

        assert_ok!(SubtensorModule::do_dissolve_network(netuid));
        assert!(!SubtensorModule::if_subnet_exist(netuid));
    })
}

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
fn dissolve_defers_cleanup_until_on_idle() {
    new_test_ext(0).execute_with(|| {
        let owner_cold = U256::from(11);
        let owner_hot = U256::from(12);
        let net = add_dynamic_network(&owner_hot, &owner_cold);

        // Set up EVM association data to verify it gets cleaned up too.
        let evm_key = sp_core::H160::from_low_u64_be(42);
        SubtensorModule::set_associated_evm_address(net, 0u16, evm_key, 1u64);
        assert!(AssociatedEvmAddress::<Test>::contains_key(net, 0u16));
        assert!(!AssociatedUidsByEvmAddress::<Test>::get(net, evm_key).is_empty());

        assert!(SubnetOwner::<Test>::contains_key(net));
        assert!(SubnetOwnerHotkey::<Test>::contains_key(net));
        assert!(NetworkRegisteredAt::<Test>::contains_key(net));
        assert!(!DissolveCleanupQueue::<Test>::get().contains(&net));

        assert_ok!(SubtensorModule::do_dissolve_network(net));

        // Network is no longer considered "existing" but data is not cleaned yet.
        assert!(!SubtensorModule::if_subnet_exist(net));
        assert!(DissolveCleanupQueue::<Test>::get().contains(&net));
        assert!(SubnetOwner::<Test>::contains_key(net));
        assert!(NetworkRegisteredAt::<Test>::contains_key(net));
        // EVM data still present before on_idle cleanup.
        assert!(AssociatedEvmAddress::<Test>::contains_key(net, 0u16));
        assert!(!AssociatedUidsByEvmAddress::<Test>::get(net, evm_key).is_empty());

        // Cleanup happens in on_idle.
        run_block_idle();
        assert!(!NetworkRegisteredAt::<Test>::contains_key(net));
        assert!(!SubnetOwner::<Test>::contains_key(net));
        assert!(!DissolveCleanupQueue::<Test>::get().contains(&net));
        // EVM data cleaned up as part of NetworkMapParameters phase.
        assert!(!AssociatedEvmAddress::<Test>::contains_key(net, 0u16));
        assert!(AssociatedUidsByEvmAddress::<Test>::get(net, evm_key).is_empty());
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
fn dissolve_clears_all_per_subnet_storages() {
    new_test_ext(0).execute_with(|| {
        let owner_cold = U256::from(123);
        let owner_hot = U256::from(456);
        let net = add_dynamic_network(&owner_hot, &owner_cold);

        // ------------------------------------------------------------------
        // Populate each storage item with a minimal value of the CORRECT type
        // ------------------------------------------------------------------
        // Core ownership / bookkeeping
        SubnetOwner::<Test>::insert(net, owner_cold);
        SubnetOwnerHotkey::<Test>::insert(net, owner_hot);
        SubnetworkN::<Test>::insert(net, 0u16);
        NetworksAdded::<Test>::insert(net, true);
        NetworkRegisteredAt::<Test>::insert(net, 0u64);

        // Consensus vectors
        Active::<Test>::insert(net, vec![true]);
        Emission::<Test>::insert(net, vec![AlphaBalance::from(1)]);
        Incentive::<Test>::insert(NetUidStorageIndex::from(net), vec![PerU16::from_parts(1)]);
        Consensus::<Test>::insert(net, vec![PerU16::from_parts(1)]);
        Dividends::<Test>::insert(net, vec![PerU16::from_parts(1)]);
        LastUpdate::<Test>::insert(NetUidStorageIndex::from(net), vec![0u64]);
        ValidatorPermit::<Test>::insert(net, vec![true]);
        ValidatorTrust::<Test>::insert(net, vec![PerU16::from_parts(1)]);

        // Per‑net params
        Tempo::<Test>::insert(net, 1u16);
        Kappa::<Test>::insert(net, 1u16);
        Difficulty::<Test>::insert(net, 1u64);

        MaxAllowedUids::<Test>::insert(net, 1u16);
        ImmunityPeriod::<Test>::insert(net, 1u16);
        ActivityCutoff::<Test>::insert(net, 1u16);
        MinAllowedWeights::<Test>::insert(net, 1u16);

        RegistrationsThisInterval::<Test>::insert(net, 1u16);
        POWRegistrationsThisInterval::<Test>::insert(net, 1u16);
        BurnRegistrationsThisInterval::<Test>::insert(net, 1u16);

        // Pool / AMM counters
        SubnetTAO::<Test>::insert(net, TaoBalance::from(1));
        SubnetAlphaInEmission::<Test>::insert(net, AlphaBalance::from(1));
        SubnetAlphaOutEmission::<Test>::insert(net, AlphaBalance::from(1));
        SubnetTaoInEmission::<Test>::insert(net, TaoBalance::from(1));
        SubnetVolume::<Test>::insert(net, 1u128);

        // Items now REMOVED (not zeroed) by dissolution
        SubnetAlphaIn::<Test>::insert(net, AlphaBalance::from(2));
        SubnetAlphaOut::<Test>::insert(net, AlphaBalance::from(3));
        SubnetProtocolAlpha::<Test>::insert(net, AlphaBalance::from(4));

        // Prefix / double-map collections
        Keys::<Test>::insert(net, 0u16, owner_hot);
        Bonds::<Test>::insert(NetUidStorageIndex::from(net), 0u16, vec![(0u16, 1u16)]);
        Weights::<Test>::insert(NetUidStorageIndex::from(net), 0u16, vec![(1u16, 1u16)]);

        // Membership entry for the SAME hotkey as Keys
        IsNetworkMember::<Test>::insert(owner_hot, net, true);

        // Token / price / provided reserves
        TokenSymbol::<Test>::insert(net, b"XX".to_vec());
        SubnetMovingPrice::<Test>::insert(net, substrate_fixed::types::I96F32::from_num(1));

        // TAO Flow
        SubnetTaoFlow::<Test>::insert(net, 0i64);
        SubnetEmaTaoFlow::<Test>::insert(net, (0u64, substrate_fixed::types::I64F64::from_num(0)));

        // Subnet locks
        TransferToggle::<Test>::insert(net, true);
        SubnetLocked::<Test>::insert(net, TaoBalance::from(1));
        LargestLocked::<Test>::insert(net, 1u64);

        // Subnet parameters & pending counters
        FirstEmissionBlockNumber::<Test>::insert(net, 1u64);
        SubnetMechanism::<Test>::insert(net, 1u16);
        NetworkRegistrationAllowed::<Test>::insert(net, true);
        NetworkPowRegistrationAllowed::<Test>::insert(net, true);
        PendingServerEmission::<Test>::insert(net, AlphaBalance::from(1));
        PendingValidatorEmission::<Test>::insert(net, AlphaBalance::from(1));
        PendingRootAlphaDivs::<Test>::insert(net, AlphaBalance::from(1));
        PendingOwnerCut::<Test>::insert(net, AlphaBalance::from(1));
        MinerBurned::<Test>::insert(net, substrate_fixed::types::U96F32::from_num(1));
        BlocksSinceLastStep::<Test>::insert(net, 1u64);
        LastMechansimStepBlock::<Test>::insert(net, 1u64);
        ServingRateLimit::<Test>::insert(net, 1u64);
        Rho::<Test>::insert(net, 1u16);
        AlphaSigmoidSteepness::<Test>::insert(net, 1i16);

        // Weights/versioning/targets/limits
        WeightsVersionKey::<Test>::insert(net, 1u64);
        MaxAllowedValidators::<Test>::insert(net, 1u16);
        AdjustmentInterval::<Test>::insert(net, 2u16);
        BondsMovingAverage::<Test>::insert(net, 1u64);
        BondsPenalty::<Test>::insert(net, 1u16);
        BondsResetOn::<Test>::insert(net, true);
        WeightsSetRateLimit::<Test>::insert(net, 1u64);
        ValidatorPruneLen::<Test>::insert(net, 1u64);
        ScalingLawPower::<Test>::insert(net, 1u16);
        TargetRegistrationsPerInterval::<Test>::insert(net, 1u16);
        AdjustmentAlpha::<Test>::insert(net, 1u64);
        CommitRevealWeightsEnabled::<Test>::insert(net, true);

        // Burn/difficulty/adjustment
        Burn::<Test>::insert(net, TaoBalance::from(1));
        MinBurn::<Test>::insert(net, TaoBalance::from(1));
        MaxBurn::<Test>::insert(net, TaoBalance::from(2));
        MinDifficulty::<Test>::insert(net, 1u64);
        MaxDifficulty::<Test>::insert(net, 2u64);
        RegistrationsThisBlock::<Test>::insert(net, 1u16);
        EMAPriceHalvingBlocks::<Test>::insert(net, 1u64);
        RAORecycledForRegistration::<Test>::insert(net, TaoBalance::from(1));

        // Feature toggles
        LiquidAlphaOn::<Test>::insert(net, true);
        Yuma3On::<Test>::insert(net, true);
        AlphaValues::<Test>::insert(net, (1u16, 2u16));
        SubtokenEnabled::<Test>::insert(net, true);
        OwnerCutAutoLockEnabled::<Test>::insert(net, true);
        ImmuneOwnerUidsLimit::<Test>::insert(net, 1u16);

        // Per‑subnet vectors / indexes
        StakeWeight::<Test>::insert(net, vec![1u16]);

        // Uid/registration
        Uids::<Test>::insert(net, owner_hot, 0u16);
        BlockAtRegistration::<Test>::insert(net, 0u16, 1u64);

        // Per‑subnet dividends
        AlphaDividendsPerSubnet::<Test>::insert(net, owner_hot, AlphaBalance::from(1));

        // Parent/child topology + takes
        ChildkeyTake::<Test>::insert(owner_hot, net, PerU16::from_parts(1));
        PendingChildKeys::<Test>::insert(net, owner_cold, (vec![(1u64, owner_hot)], 1u64));
        ChildKeys::<Test>::insert(owner_cold, net, vec![(1u64, owner_hot)]);
        ParentKeys::<Test>::insert(owner_hot, net, vec![(1u64, owner_cold)]);

        // Hotkey swap timestamp for subnet
        LastHotkeySwapOnNetuid::<Test>::insert(net, owner_cold, 1u64);

        // Axon/prometheus tx key timing (NMap) — ***correct key-tuple insertion***
        TransactionKeyLastBlock::<Test>::insert((owner_hot, net, 1u16), 1u64);

        // EVM association indexed by (netuid, uid)
        SubtensorModule::set_associated_evm_address(net, 0u16, sp_core::H160::zero(), 1u64);

        // (Optional) subnet -> lease link
        SubnetUidToLeaseId::<Test>::insert(net, 42u32);

        // ------------------------------------------------------------------
        // Dissolve
        // ------------------------------------------------------------------
        assert_ok!(SubtensorModule::do_dissolve_network(net));
        run_block_idle();

        // ------------------------------------------------------------------
        // Items that must be COMPLETELY REMOVED
        // ------------------------------------------------------------------
        assert!(!SubnetOwner::<Test>::contains_key(net));
        assert!(!SubnetOwnerHotkey::<Test>::contains_key(net));
        assert!(!SubnetworkN::<Test>::contains_key(net));
        assert!(!NetworksAdded::<Test>::contains_key(net));
        assert!(!NetworkRegisteredAt::<Test>::contains_key(net));

        // Consensus vectors removed
        assert!(!Active::<Test>::contains_key(net));
        assert!(!Emission::<Test>::contains_key(net));
        assert!(!Incentive::<Test>::contains_key(NetUidStorageIndex::from(
            net
        )));
        assert!(!Consensus::<Test>::contains_key(net));
        assert!(!Dividends::<Test>::contains_key(net));
        assert!(!LastUpdate::<Test>::contains_key(NetUidStorageIndex::from(
            net
        )));

        assert!(!ValidatorPermit::<Test>::contains_key(net));
        assert!(!ValidatorTrust::<Test>::contains_key(net));

        // Per‑net params removed
        assert!(!Tempo::<Test>::contains_key(net));
        assert!(!Kappa::<Test>::contains_key(net));
        assert!(!Difficulty::<Test>::contains_key(net));

        assert!(!MaxAllowedUids::<Test>::contains_key(net));
        assert!(!ImmunityPeriod::<Test>::contains_key(net));
        assert!(!ActivityCutoff::<Test>::contains_key(net));
        assert!(!MinAllowedWeights::<Test>::contains_key(net));

        assert!(!RegistrationsThisInterval::<Test>::contains_key(net));
        assert!(!POWRegistrationsThisInterval::<Test>::contains_key(net));
        assert!(!BurnRegistrationsThisInterval::<Test>::contains_key(net));

        // Pool / AMM counters removed
        assert!(!SubnetTAO::<Test>::contains_key(net));
        assert!(!SubnetAlphaInEmission::<Test>::contains_key(net));
        assert!(!SubnetAlphaOutEmission::<Test>::contains_key(net));
        assert!(!SubnetTaoInEmission::<Test>::contains_key(net));
        assert!(!SubnetVolume::<Test>::contains_key(net));
        assert!(!pallet_subtensor_swap::BalancerTaoReservoir::<Test>::contains_key(net));
        assert!(!pallet_subtensor_swap::BalancerAlphaReservoir::<Test>::contains_key(net));

        // TAO Flow
        assert!(!SubnetTaoFlow::<Test>::contains_key(net));
        assert!(!SubnetEmaTaoFlow::<Test>::contains_key(net));

        // These are now REMOVED
        assert!(!SubnetAlphaIn::<Test>::contains_key(net));
        assert!(!SubnetAlphaOut::<Test>::contains_key(net));
        assert!(!SubnetProtocolAlpha::<Test>::contains_key(net));

        // Collections fully cleared
        assert!(Keys::<Test>::iter_prefix(net).next().is_none());
        assert!(
            Bonds::<Test>::iter_prefix(NetUidStorageIndex::from(net))
                .next()
                .is_none()
        );
        assert!(
            Weights::<Test>::iter_prefix(NetUidStorageIndex::from(net))
                .next()
                .is_none()
        );
        assert!(!IsNetworkMember::<Test>::contains_key(owner_hot, net));

        // Token / price / provided reserves
        assert!(!TokenSymbol::<Test>::contains_key(net));
        assert!(!SubnetMovingPrice::<Test>::contains_key(net));

        // Subnet locks
        assert!(!TransferToggle::<Test>::contains_key(net));
        assert!(!SubnetLocked::<Test>::contains_key(net));
        assert!(!LargestLocked::<Test>::contains_key(net));

        // Subnet parameters & pending counters
        assert!(!FirstEmissionBlockNumber::<Test>::contains_key(net));
        assert!(!SubnetMechanism::<Test>::contains_key(net));
        assert!(!NetworkRegistrationAllowed::<Test>::contains_key(net));
        assert!(!NetworkPowRegistrationAllowed::<Test>::contains_key(net));
        assert!(!PendingServerEmission::<Test>::contains_key(net));
        assert!(!PendingValidatorEmission::<Test>::contains_key(net));
        assert!(!PendingRootAlphaDivs::<Test>::contains_key(net));
        assert!(!PendingOwnerCut::<Test>::contains_key(net));
        assert!(!MinerBurned::<Test>::contains_key(net));
        assert!(!BlocksSinceLastStep::<Test>::contains_key(net));
        assert!(!LastMechansimStepBlock::<Test>::contains_key(net));
        assert!(!ServingRateLimit::<Test>::contains_key(net));
        assert!(!Rho::<Test>::contains_key(net));
        assert!(!AlphaSigmoidSteepness::<Test>::contains_key(net));

        // Weights/versioning/targets/limits
        assert!(!WeightsVersionKey::<Test>::contains_key(net));
        assert!(!MaxAllowedValidators::<Test>::contains_key(net));
        assert!(!BondsMovingAverage::<Test>::contains_key(net));
        assert!(!BondsPenalty::<Test>::contains_key(net));
        assert!(!BondsResetOn::<Test>::contains_key(net));
        assert!(!WeightsSetRateLimit::<Test>::contains_key(net));
        assert!(!ValidatorPruneLen::<Test>::contains_key(net));
        assert!(!ScalingLawPower::<Test>::contains_key(net));
        assert!(!TargetRegistrationsPerInterval::<Test>::contains_key(net));
        assert!(!CommitRevealWeightsEnabled::<Test>::contains_key(net));

        // Burn/difficulty/adjustment
        assert!(!Burn::<Test>::contains_key(net));
        assert!(!MinBurn::<Test>::contains_key(net));
        assert!(!MaxBurn::<Test>::contains_key(net));
        assert!(!MinDifficulty::<Test>::contains_key(net));
        assert!(!MaxDifficulty::<Test>::contains_key(net));
        assert!(!RegistrationsThisBlock::<Test>::contains_key(net));
        assert!(!EMAPriceHalvingBlocks::<Test>::contains_key(net));
        assert!(!RAORecycledForRegistration::<Test>::contains_key(net));

        // Feature toggles
        assert!(!LiquidAlphaOn::<Test>::contains_key(net));
        assert!(!Yuma3On::<Test>::contains_key(net));
        assert!(!AlphaValues::<Test>::contains_key(net));
        assert!(!SubtokenEnabled::<Test>::contains_key(net));
        assert!(!OwnerCutAutoLockEnabled::<Test>::contains_key(net));
        assert!(!ImmuneOwnerUidsLimit::<Test>::contains_key(net));

        // Per‑subnet vectors / indexes
        assert!(!StakeWeight::<Test>::contains_key(net));

        // Uid/registration
        assert!(Uids::<Test>::get(net, owner_hot).is_none());
        assert!(!BlockAtRegistration::<Test>::contains_key(net, 0u16));

        // Per‑subnet dividends
        assert!(!AlphaDividendsPerSubnet::<Test>::contains_key(
            net, owner_hot
        ));

        // Parent/child topology + takes
        assert!(!ChildkeyTake::<Test>::contains_key(owner_hot, net));
        assert!(!PendingChildKeys::<Test>::contains_key(net, owner_cold));
        assert!(!ChildKeys::<Test>::contains_key(owner_cold, net));
        assert!(!ParentKeys::<Test>::contains_key(owner_hot, net));

        // Hotkey swap timestamp for subnet
        assert!(!LastHotkeySwapOnNetuid::<Test>::contains_key(
            net, owner_cold
        ));

        // Axon/prometheus tx key timing (NMap) — ValueQuery (defaults to 0)
        assert_eq!(
            TransactionKeyLastBlock::<Test>::get((owner_hot, net, 1u16)),
            0u64
        );

        // EVM association
        assert!(AssociatedEvmAddress::<Test>::get(net, 0u16).is_none());
        assert!(AssociatedUidsByEvmAddress::<Test>::get(net, sp_core::H160::zero()).is_empty());

        // Subnet -> lease link
        assert!(!SubnetUidToLeaseId::<Test>::contains_key(net));

        // ------------------------------------------------------------------
        // Final subnet removal confirmation
        // ------------------------------------------------------------------
        assert!(!SubtensorModule::if_subnet_exist(net));
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

#[test]
fn prune_none_with_no_networks() {
    new_test_ext(0).execute_with(|| {
        assert_eq!(SubtensorModule::get_network_to_prune(), None);
    });
}

#[test]
fn prune_none_when_all_networks_immune() {
    new_test_ext(0).execute_with(|| {
        // two fresh networks → still inside immunity window
        let n1 = add_dynamic_network(&U256::from(2), &U256::from(1));
        let _n2 = add_dynamic_network(&U256::from(4), &U256::from(3));

        // emissions don’t matter while immune
        Emission::<Test>::insert(n1, vec![AlphaBalance::from(10)]);

        assert_eq!(SubtensorModule::get_network_to_prune(), None);
    });
}

#[test]
fn prune_selects_network_with_lowest_price() {
    new_test_ext(0).execute_with(|| {
        let n1 = add_dynamic_network(&U256::from(20), &U256::from(10));
        let n2 = add_dynamic_network(&U256::from(40), &U256::from(30));

        // make both networks eligible (past immunity)
        let imm = SubtensorModule::get_network_immunity_period();
        System::set_block_number(imm + 10);

        // n1 has lower price → should be pruned
        SubnetMovingPrice::<Test>::insert(n1, I96F32::from_num(1));
        SubnetMovingPrice::<Test>::insert(n2, I96F32::from_num(10));

        assert_eq!(SubtensorModule::get_network_to_prune(), Some(n1));
    });
}

#[test]
fn prune_ignores_immune_network_even_if_lower_price() {
    new_test_ext(0).execute_with(|| {
        // create mature network n1 first
        let n1 = add_dynamic_network(&U256::from(22), &U256::from(11));

        let imm = SubtensorModule::get_network_immunity_period();
        System::set_block_number(imm + 5); // advance → n1 now mature

        // create second network n2 *inside* immunity
        let n2 = add_dynamic_network(&U256::from(44), &U256::from(33));

        // prices: n2 lower but immune; n1 must be selected
        SubnetMovingPrice::<Test>::insert(n1, I96F32::from_num(5));
        SubnetMovingPrice::<Test>::insert(n2, I96F32::from_num(1));

        System::set_block_number(imm + 10); // still immune for n2
        assert_eq!(SubtensorModule::get_network_to_prune(), Some(n1));
    });
}

#[test]
fn prune_tie_on_price_earlier_registration_wins() {
    new_test_ext(0).execute_with(|| {
        // n1 registered first
        let n1 = add_dynamic_network(&U256::from(66), &U256::from(55));

        // advance 1 block, then register n2 (later timestamp)
        System::set_block_number(1);
        let n2 = add_dynamic_network(&U256::from(88), &U256::from(77));

        // push past immunity for both
        let imm = SubtensorModule::get_network_immunity_period();
        System::set_block_number(imm + 20);

        // identical prices → tie; earlier (n1) must be chosen
        SubnetMovingPrice::<Test>::insert(n1, I96F32::from_num(7));
        SubnetMovingPrice::<Test>::insert(n2, I96F32::from_num(7));

        assert_eq!(SubtensorModule::get_network_to_prune(), Some(n1));
    });
}

/// Two mature subnets whose prices make `low` the eviction candidate. `low`'s owner is funded
/// for one match; tests that care about an unfunded owner drain them.
fn refusal_setup(seed: u64) -> (NetUid, NetUid, U256, U256) {
    SubnetLimit::<Test>::put(2u16);

    let low_cold = U256::from(seed);
    let low = add_dynamic_network(&U256::from(seed.saturating_add(1)), &low_cold);
    let high = add_dynamic_network(
        &U256::from(seed.saturating_add(3)),
        &U256::from(seed.saturating_add(2)),
    );

    let imm = SubtensorModule::get_network_immunity_period();
    System::set_block_number(imm.saturating_add(10));

    SubnetMovingPrice::<Test>::insert(low, I96F32::from_num(1));
    SubnetMovingPrice::<Test>::insert(high, I96F32::from_num(10));
    assert_eq!(SubtensorModule::get_network_to_prune(), Some(low));

    let price = SubtensorModule::get_network_lock_cost();
    add_balance_to_coldkey_account(&low_cold, price.into());
    TotalIssuance::<Test>::mutate(|total| *total = total.saturating_add(price));

    let registrant = U256::from(seed.saturating_add(4));
    let funding = price.saturating_mul(10.into());
    add_balance_to_coldkey_account(&registrant, funding.into());
    TotalIssuance::<Test>::mutate(|total| *total = total.saturating_add(funding));

    (low, high, low_cold, registrant)
}

fn register_from(registrant: U256, hotkey: u64) -> DispatchResult {
    frame_support::storage::with_storage_layer(|| {
        SubtensorModule::do_register_network(
            RuntimeOrigin::signed(registrant),
            &U256::from(hotkey),
            1,
            None,
        )
    })
}

/// The core of the mechanism: reaching the cap no longer evicts anybody. It records what the
/// challenger locked and gives the owner a window to answer it.
#[test]
fn a_challenge_opens_a_window_instead_of_evicting() {
    new_test_ext(0).execute_with(|| {
        let (low, high, _low_cold, registrant) = refusal_setup(4000);
        let price = SubtensorModule::get_network_lock_cost();

        assert_ok!(register_from(registrant, 4005));

        // Nobody lost a slot.
        assert!(NetworksAdded::<Test>::get(low));
        assert!(NetworksAdded::<Test>::get(high));
        assert!(DissolveCleanupQueue::<Test>::get().is_empty());

        let window = SubnetRefusalWindow::<Test>::get(low).unwrap();
        let (offer, expires_at) = (window.offer, window.expires_at);
        assert_eq!(offer, price);
        assert_eq!(
            expires_at,
            System::block_number().saturating_add(SubtensorModule::FIRST_REFUSAL_WINDOW)
        );

        // The challenger is in line rather than turned away, at the price they were quoted.
        let queue = NetworkRegistrationQueue::<Test>::get();
        assert_eq!(queue.len(), 1);
        assert_eq!(queue[0].coldkey, registrant);
        assert_eq!(queue[0].lock_amount, price);
    });
}

/// The right of first refusal itself: the owner is asked for the challenger's own figure, never
/// one of their own naming, so answering cannot cost more than a real buyer just committed.
#[test]
fn the_owner_pays_no_more_than_the_challenger_committed() {
    new_test_ext(0).execute_with(|| {
        let (low, high, low_cold, registrant) = refusal_setup(4010);

        assert_ok!(register_from(registrant, 4015));
        let offer = SubnetRefusalWindow::<Test>::get(low).unwrap().offer;
        let balance_before = SubtensorModule::get_coldkey_balance(&low_cold);
        let issuance_before = TotalIssuance::<Test>::get();

        assert_ok!(SubtensorModule::exercise_first_refusal(
            RuntimeOrigin::signed(low_cold),
            low
        ));

        // The owner pays the challenger's own figure, and it is burned rather than banked.
        let paid = balance_before.saturating_sub(SubtensorModule::get_coldkey_balance(&low_cold));
        assert_eq!(paid, offer.into());
        assert_eq!(
            issuance_before.saturating_sub(TotalIssuance::<Test>::get()),
            offer
        );

        // The refused challenger is out of the queue with their lock released.
        assert!(NetworkRegistrationQueue::<Test>::get().is_empty());
        assert_eq!(
            Balances::usable_balance(registrant),
            Balances::free_balance(registrant)
        );

        assert!(NetworksAdded::<Test>::get(low));
        assert!(NetworksAdded::<Test>::get(high));
        assert!(!SubnetRefusalWindow::<Test>::contains_key(low));
        assert!(NetworkImmuneUntil::<Test>::get(low) > System::block_number());
    });
}

/// Why the decision is a signed call in a window rather than a standing authorization. An owner
/// holding nothing when the challenge lands can still answer it, so there is no balance for a
/// challenger to read and no moment at which their intent is on chain. Without this, somebody
/// with no interest in a slot could wait for the decaying price to pass under a visible balance
/// and register purely to make the owner burn it.
#[test]
fn an_owner_who_was_broke_when_challenged_can_still_answer() {
    new_test_ext(0).execute_with(|| {
        let (low, _high, low_cold, registrant) = refusal_setup(4020);

        // Nothing to see, and nothing to extract.
        Balances::make_free_balance_be(&low_cold, TaoBalance::from(0u64));
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&low_cold),
            TaoBalance::from(0u64)
        );

        assert_ok!(register_from(registrant, 4025));
        let offer = SubnetRefusalWindow::<Test>::get(low).unwrap().offer;

        // Funded only after seeing a real offer. The extra existential deposit is there because
        // no account may spend itself out of existence, not because the price is any higher.
        let funded = offer.saturating_add(ExistentialDeposit::get());
        add_balance_to_coldkey_account(&low_cold, funded.into());
        TotalIssuance::<Test>::mutate(|total| *total = total.saturating_add(funded));

        assert_ok!(SubtensorModule::exercise_first_refusal(
            RuntimeOrigin::signed(low_cold),
            low
        ));
        assert!(NetworksAdded::<Test>::get(low));
        assert!(NetworkImmuneUntil::<Test>::get(low) > System::block_number());
    });
}

/// Declining is allowed and costs the owner nothing. The next registration takes the slot.
#[test]
fn an_unanswered_window_lapses_and_the_slot_is_taken() {
    new_test_ext(0).execute_with(|| {
        let (low, high, low_cold, registrant) = refusal_setup(4030);

        assert_ok!(register_from(registrant, 4035));
        let expires_at = SubnetRefusalWindow::<Test>::get(low).unwrap().expires_at;
        let balance_before = SubtensorModule::get_coldkey_balance(&low_cold);

        System::set_block_number(expires_at.saturating_add(1));
        assert_ok!(register_from(registrant, 4036));

        assert!(!SubnetRefusalWindow::<Test>::contains_key(low));
        assert!(!DissolveCleanupQueue::<Test>::get().is_empty());
        assert!(NetworksAdded::<Test>::get(high));
        // Letting it lapse is free.
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&low_cold),
            balance_before
        );
    });
}

/// A challenge on one subnet does not reprice a window already open on another.
///
/// The scan passes over a subnet whose owner is still deciding, so two windows can stand at once
/// and the second registration moves the lock cost while the first owner is still inside their
/// deadline. The offer is recorded when the window opens, so that owner answers the figure they
/// were shown rather than a number somebody else's registration set afterwards.
///
/// One queue entry per outstanding challenge is the honest bound here. It used to be one
/// chain-wide, enforced by refusing the second registration outright, and that turned a single
/// lock into a hold on every at-cap registration.
#[test]
fn a_challenge_elsewhere_does_not_reprice_an_open_window() {
    new_test_ext(0).execute_with(|| {
        let (low, high, low_cold, registrant) = refusal_setup(4040);

        let big = SubtensorModule::get_network_lock_cost().saturating_mul(10_000.into());
        add_balance_to_coldkey_account(&registrant, big.into());
        add_balance_to_coldkey_account(&low_cold, big.into());
        TotalIssuance::<Test>::mutate(|t| *t = t.saturating_add(big.saturating_mul(2.into())));

        assert_ok!(register_from(registrant, 4045));
        let first_window = SubnetRefusalWindow::<Test>::get(low).unwrap();

        // The other subnet is a live eviction candidate with nothing standing on it.
        assert!(NetworksAdded::<Test>::get(high));
        assert!(!SubnetRefusalWindow::<Test>::contains_key(high));

        System::set_block_number(System::block_number().saturating_add(10));

        // Diverted to that candidate, not refused.
        assert_ok!(register_from(registrant, 4046));
        assert!(SubnetRefusalWindow::<Test>::contains_key(high));
        assert_eq!(NetworkRegistrationQueue::<Test>::get().len(), 2);
        assert_eq!(SubnetRefusalWindow::<Test>::get(low).unwrap(), first_window);

        // And the first owner is charged the figure their window recorded.
        let balance_before = SubtensorModule::get_coldkey_balance(&low_cold);
        assert_ok!(SubtensorModule::exercise_first_refusal(
            RuntimeOrigin::signed(low_cold),
            low
        ));
        let paid = balance_before.saturating_sub(SubtensorModule::get_coldkey_balance(&low_cold));
        assert_eq!(paid, first_window.offer.into());
    });
}

/// What bounds the queue.
///
/// A challenge is the one path that appends to `NetworkRegistrationQueue` without dissolving
/// anything, so the "every entry is paid for by a teardown" argument that bounds the unchallenged
/// path does not cover it. Answering refunds and dequeues the challenger, so the append is undone
/// by the same call that ends the challenge and the queue is back where it started. Answering also
/// makes the subnet immune, so it leaves the eviction pool; once nothing is evictable
/// `get_network_to_prune` yields `None` and registrations are refused outright rather than queued.
///
/// `matched_challengers_do_not_accumulate_across_immunity_cycles` covers the same property once
/// the bought immunity has lapsed and the subnet is back in the pool.
#[test]
fn challenges_cannot_grow_the_queue() {
    new_test_ext(0).execute_with(|| {
        let (low, high, low_cold, registrant) = refusal_setup(4090);

        // `refusal_setup` funds `low`'s owner for one match; the other owner needs the same.
        let high_cold = U256::from(4092u64);
        let funding = SubtensorModule::get_network_lock_cost().saturating_mul(8.into());
        add_balance_to_coldkey_account(&high_cold, funding.into());
        TotalIssuance::<Test>::mutate(|total| *total = total.saturating_add(funding));

        // The candidate is challenged and its owner answers, so it leaves the eviction pool.
        assert_ok!(register_from(registrant, 4095));
        assert_ok!(SubtensorModule::exercise_first_refusal(
            RuntimeOrigin::signed(low_cold),
            low
        ));
        assert!(NetworkRegistrationQueue::<Test>::get().is_empty());

        // Only the other subnet is left to challenge, and its owner answers too.
        assert_ok!(register_from(registrant, 4096));
        assert!(SubnetRefusalWindow::<Test>::contains_key(high));
        assert_ok!(SubtensorModule::exercise_first_refusal(
            RuntimeOrigin::signed(high_cold),
            high
        ));
        assert!(NetworkRegistrationQueue::<Test>::get().is_empty());

        // The pool is empty, so the next registration is turned away instead of queued.
        assert_eq!(SubtensorModule::get_network_to_prune(), None);
        assert_noop!(
            register_from(registrant, 4097),
            Error::<Test>::SubnetLimitReached
        );
        assert!(NetworkRegistrationQueue::<Test>::get().is_empty());
    });
}

/// The other place a stale figure is settled, and the same rule as the queued path.
///
/// The offer is quoted when the window opens and can be a full `FIRST_REFUSAL_WINDOW` old by the
/// time it is answered. Registrations landing on other subnets in the meantime move the price up.
/// Letting the owner's stale figure reset the accumulator down to it would hand every incumbent a
/// way to cancel an escalation they took no part in, for the price they were quoted a day earlier.
#[test]
fn answering_an_old_challenge_does_not_lower_the_lock_cost() {
    new_test_ext(0).execute_with(|| {
        let (low, _high, low_cold, registrant) = refusal_setup(4100);

        assert_ok!(register_from(registrant, 4105));
        let offer = SubnetRefusalWindow::<Test>::get(low).unwrap().offer;

        // Demand elsewhere moves the price up while this owner is still deciding.
        let raised = offer.saturating_mul(3.into());
        SubtensorModule::set_network_last_lock(raised);

        let balance_before = SubtensorModule::get_coldkey_balance(&low_cold);
        assert_ok!(SubtensorModule::exercise_first_refusal(
            RuntimeOrigin::signed(low_cold),
            low
        ));

        // The owner is still charged only the figure they were shown.
        let paid = balance_before.saturating_sub(SubtensorModule::get_coldkey_balance(&low_cold));
        assert_eq!(paid, offer.into());

        // The accumulator holds where the market left it.
        assert_eq!(SubtensorModule::get_network_last_lock(), raised);
    });
}

/// A window is only worth answering while the challenger who opened it is still waiting.
///
/// If that registration is served from some other slot that came free, nothing is threatening this
/// subnet any more. The owner must not be able to burn the offer for immunity nobody was
/// contesting, and the next registration to arrive should open a fresh window at its own price
/// rather than evict on the strength of a challenge that has already dissolved.
#[test]
fn a_window_whose_challenger_left_the_queue_cannot_be_answered() {
    new_test_ext(0).execute_with(|| {
        let (low, _high, low_cold, registrant) = refusal_setup(4040);

        assert_ok!(register_from(registrant, 4045));
        assert!(SubnetRefusalWindow::<Test>::contains_key(low));

        // The challenger's registration is served from somewhere else.
        NetworkRegistrationQueue::<Test>::set(vec![]);

        assert_noop!(
            SubtensorModule::exercise_first_refusal(RuntimeOrigin::signed(low_cold), low),
            Error::<Test>::NoChallengeToAnswer
        );
    });
}

/// The same rule, on the eviction side and after the deadline has also passed.
///
/// A challenger leaving the queue ends the challenge, and it ends it whether or not the window has
/// since expired. If expiry were checked first, a subnet could be evicted on the strength of a
/// challenge that had already dissolved: the owner would be pruned for declining an offer that
/// withdrew itself, and the registration actually taking the slot would never have put its own
/// price to them.
#[test]
fn a_lapsed_window_whose_challenger_left_does_not_evict() {
    new_test_ext(0).execute_with(|| {
        let (low, high, _low_cold, registrant) = refusal_setup(4050);

        assert_ok!(register_from(registrant, 4055));
        let expires_at = SubnetRefusalWindow::<Test>::get(low).unwrap().expires_at;

        // Served from a slot that came free elsewhere, so nothing contests `low` any more. Only
        // then does the deadline pass.
        NetworkRegistrationQueue::<Test>::set(vec![]);
        System::set_block_number(expires_at.saturating_add(1));

        assert_ok!(register_from(registrant, 4056));

        assert!(NetworksAdded::<Test>::get(low));
        assert!(NetworksAdded::<Test>::get(high));
        assert!(DissolveCleanupQueue::<Test>::get().is_empty());

        let fresh = SubnetRefusalWindow::<Test>::get(low).expect("a fresh window at its own price");
        assert!(fresh.expires_at > expires_at);
    });
}

/// Serving the cheap head of the queue must not undo a refusal that raised the price.
///
/// A queued registration settles at the price it locked, which can be far behind the market by the
/// time a slot reaches it. If that figure were written straight back into `NetworkLastLockCost`,
/// the escalation refusals are supposed to produce would collapse the moment the oldest entry is
/// served, which would make answering a challenge worth much less than it looks.
#[test]
fn serving_an_old_queued_registration_does_not_lower_the_lock_cost() {
    new_test_ext(0).execute_with(|| {
        let (low, _high, low_cold, registrant) = refusal_setup(4040);

        assert_ok!(register_from(registrant, 4045));
        let offer = SubnetRefusalWindow::<Test>::get(low).unwrap().offer;

        assert_ok!(SubtensorModule::exercise_first_refusal(
            RuntimeOrigin::signed(low_cold),
            low
        ));
        assert_eq!(SubtensorModule::get_network_last_lock(), offer);

        // That challenger was refunded and dequeued, so the entry under test is the next one: a
        // second registration, which lands on the other subnet now that `low` is immune.
        assert_ok!(register_from(registrant, 4046));

        // It is served from a slot that came free. Raising the limit stands in for the dissolution
        // that would have freed one; what is under test is the price the serving writes back, not
        // the slot accounting that got it there.
        SubnetLimit::<Test>::put(3u16);

        // The market moves further up while the entry waits, so the figure it locked is now stale.
        let market = offer.saturating_mul(4.into());
        SubtensorModule::set_network_last_lock(market);

        let queued = NetworkRegistrationQueue::<Test>::get();
        let entry = queued
            .first()
            .expect("the second challenger is queued")
            .clone();
        assert!(entry.lock_amount < market);

        assert_ok!(SubtensorModule::set_new_network_state(
            &entry.coldkey,
            &entry.hotkey,
            entry.mechid,
            entry.identity.clone(),
            entry.lock_amount,
            entry.median_subnet_alpha_price,
            Some(entry.lock_id),
        ));

        assert_eq!(SubtensorModule::get_network_last_lock(), market);
    });
}

/// Answering is the same demand for a scarce slot as a registration that lands, so it moves the
/// price ladder the same way. Otherwise an incumbent holds a slot at the floor forever.
#[test]
fn answering_doubles_the_next_registration_price() {
    new_test_ext(0).execute_with(|| {
        let (low, _high, low_cold, registrant) = refusal_setup(4050);

        assert_ok!(register_from(registrant, 4055));
        let offer = SubnetRefusalWindow::<Test>::get(low).unwrap().offer;

        assert_ok!(SubtensorModule::exercise_first_refusal(
            RuntimeOrigin::signed(low_cold),
            low
        ));

        assert_eq!(SubtensorModule::get_network_last_lock(), offer);
        assert_eq!(
            SubtensorModule::get_network_last_lock_block(),
            System::block_number()
        );
        assert_eq!(
            SubtensorModule::get_network_lock_cost(),
            offer.saturating_mul(2.into())
        );
    });
}

/// Recycled, not pooled. `SubnetTAO` pays out pro rata on dissolution and the owner is usually
/// the largest alpha holder, so pooling would hand the payment back to the payer.
#[test]
fn answering_recycles_the_price_instead_of_pooling_it() {
    new_test_ext(0).execute_with(|| {
        let (low, _high, low_cold, registrant) = refusal_setup(4060);

        assert_ok!(register_from(registrant, 4065));
        let offer = SubnetRefusalWindow::<Test>::get(low).unwrap().offer;
        let issuance_before = TotalIssuance::<Test>::get();
        let subnet_tao_before = SubnetTAO::<Test>::get(low);
        let locked_before = SubnetLocked::<Test>::get(low);

        assert_ok!(SubtensorModule::exercise_first_refusal(
            RuntimeOrigin::signed(low_cold),
            low
        ));

        assert_eq!(
            issuance_before.saturating_sub(TotalIssuance::<Test>::get()),
            offer
        );
        assert_eq!(SubnetTAO::<Test>::get(low), subnet_tao_before);
        assert_eq!(SubnetLocked::<Test>::get(low), locked_before);
    });
}

/// Inside the period it paid for, the subnet is not a candidate at all, so the next registration
/// asks the next subnet rather than the same owner again.
#[test]
fn an_owner_is_not_challenged_again_inside_the_immunity_period() {
    new_test_ext(0).execute_with(|| {
        let (low, high, low_cold, registrant) = refusal_setup(4070);

        assert_ok!(register_from(registrant, 4075));
        assert_ok!(SubtensorModule::exercise_first_refusal(
            RuntimeOrigin::signed(low_cold),
            low
        ));

        assert_eq!(SubtensorModule::get_network_to_prune(), Some(high));
        assert_ok!(register_from(registrant, 4076));
        assert!(!SubnetRefusalWindow::<Test>::contains_key(low));
        assert!(SubnetRefusalWindow::<Test>::contains_key(high));

        // Back in line once the cover lapses.
        System::set_block_number(NetworkImmuneUntil::<Test>::get(low).saturating_add(1));
        assert_eq!(SubtensorModule::get_network_to_prune(), Some(low));
    });
}

/// A zero `NetworkImmunityPeriod` switches immunity off protocol-wide, so there is nothing to
/// buy and no point parking a registration behind a window that buys nothing.
#[test]
fn a_zero_immunity_period_prunes_without_opening_a_window() {
    new_test_ext(0).execute_with(|| {
        let (low, _high, _low_cold, registrant) = refusal_setup(4080);
        NetworkImmunityPeriod::<Test>::put(0u64);

        assert_ok!(register_from(registrant, 4085));

        assert!(!SubnetRefusalWindow::<Test>::contains_key(low));
        assert!(!DissolveCleanupQueue::<Test>::get().is_empty());
    });
}

/// The owner gets the immunity they were promised when the window opened, not whatever the
/// parameter happens to say when they pay.
///
/// Reading `NetworkImmunityPeriod` at exercise time instead would let governance lowering it
/// mid-window turn a full-price answer into nothing: the payment is burned, `immune_until` is set
/// to the current block, and `current_block < immune_until` is false immediately, so the owner
/// buys zero protection and can be challenged again in the same block.
#[test]
fn shortening_the_immunity_period_mid_window_does_not_shrink_what_was_bought() {
    new_test_ext(0).execute_with(|| {
        let (low, _high, low_cold, registrant) = refusal_setup(4100);
        let promised = SubtensorModule::get_network_immunity_period();
        assert!(promised > 0);

        assert_ok!(register_from(registrant, 4105));
        assert!(SubnetRefusalWindow::<Test>::contains_key(low));

        // Governance switches immunity off while the owner is deciding.
        NetworkImmunityPeriod::<Test>::put(0u64);

        let at = System::block_number();
        assert_ok!(SubtensorModule::exercise_first_refusal(
            RuntimeOrigin::signed(low_cold),
            low
        ));

        assert_eq!(
            NetworkImmuneUntil::<Test>::get(low),
            at.saturating_add(promised)
        );
    });
}

/// Answering after the window shuts is refused, so a lapsed right cannot be revived by an owner
/// who notices late and races the next registration.
#[test]
fn answering_after_the_window_closes_fails() {
    new_test_ext(0).execute_with(|| {
        let (low, _high, low_cold, registrant) = refusal_setup(4090);

        assert_ok!(register_from(registrant, 4095));
        let expires_at = SubnetRefusalWindow::<Test>::get(low).unwrap().expires_at;
        System::set_block_number(expires_at.saturating_add(1));

        assert_err!(
            SubtensorModule::exercise_first_refusal(RuntimeOrigin::signed(low_cold), low),
            Error::<Test>::NoChallengeToAnswer
        );
    });
}

/// Nothing to answer means nothing to pay. Without this an owner could buy immunity whenever they
/// liked and move everybody else's price with it.
#[test]
fn answering_without_a_challenge_fails() {
    new_test_ext(0).execute_with(|| {
        let (low, _high, low_cold, _registrant) = refusal_setup(4100);

        assert_err!(
            SubtensorModule::exercise_first_refusal(RuntimeOrigin::signed(low_cold), low),
            Error::<Test>::NoChallengeToAnswer
        );
        assert_eq!(NetworkImmuneUntil::<Test>::get(low), 0);
    });
}

/// Answering for someone else's subnet would be a way to burn their TAO.
#[test]
fn only_the_owner_can_answer_a_challenge() {
    new_test_ext(0).execute_with(|| {
        let (low, _high, _low_cold, registrant) = refusal_setup(4110);

        assert_ok!(register_from(registrant, 4115));

        assert_err!(
            SubtensorModule::exercise_first_refusal(RuntimeOrigin::signed(U256::from(4199)), low),
            DispatchError::BadOrigin
        );
        assert!(SubnetRefusalWindow::<Test>::contains_key(low));
    });
}

#[test]
fn answering_requires_an_existing_subnet() {
    new_test_ext(0).execute_with(|| {
        assert_err!(
            SubtensorModule::exercise_first_refusal(
                RuntimeOrigin::signed(U256::from(4120)),
                NetUid::from(999)
            ),
            Error::<Test>::SubnetNotExists
        );
    });
}

/// A netuid handed to somebody else must not carry the previous owner's cover or their open
/// window.
#[test]
fn a_reused_netuid_inherits_neither_immunity_nor_a_window() {
    new_test_ext(0).execute_with(|| {
        let (low, _high, low_cold, registrant) = refusal_setup(4130);

        assert_ok!(register_from(registrant, 4135));
        assert_ok!(SubtensorModule::exercise_first_refusal(
            RuntimeOrigin::signed(low_cold),
            low
        ));
        assert!(NetworkImmuneUntil::<Test>::get(low) > 0);

        let mut meter = WeightMeter::new();
        SubtensorModule::remove_network_parameters(low, &mut meter);

        assert_eq!(NetworkImmuneUntil::<Test>::get(low), 0);
        assert!(!SubnetRefusalWindow::<Test>::contains_key(low));
    });
}

/// A registration that fails for any other reason must not leave a window behind, since dispatch
/// unwinds the whole call and the challenge it recorded no longer exists.
#[test]
fn a_failed_registration_leaves_no_window() {
    new_test_ext(0).execute_with(|| {
        let (low, _high, _low_cold, registrant) = refusal_setup(4140);

        // Broke registrant: step 6's affordability check fails after step 5 opened the window.
        Balances::make_free_balance_be(&registrant, TaoBalance::from(0u64));

        assert_err!(
            register_from(registrant, 4145),
            Error::<Test>::CannotAffordLockCost
        );
        assert!(!SubnetRefusalWindow::<Test>::contains_key(low));
    });
}

/// The way out of the queue. A registration that has not been served is the registrant's to
/// withdraw, and withdrawing has to return the lock, not merely drop the entry.
#[test]
fn a_queued_registrant_can_withdraw_and_get_their_lock_back() {
    new_test_ext(0).execute_with(|| {
        let (_low, _high, _low_cold, registrant) = refusal_setup(4150);

        let lock_id = NetworkRegistrationLockId::<Test>::get();
        let mut identifier = [0u8; 8];
        identifier[..4].copy_from_slice(b"rglk");
        identifier[4..8].copy_from_slice(&lock_id.to_le_bytes());

        assert_ok!(register_from(registrant, 4155));
        let free_before = pallet_balances::Pallet::<Test>::free_balance(registrant);
        assert!(
            pallet_balances::Locks::<Test>::get(registrant)
                .iter()
                .any(|lock| lock.id == identifier)
        );

        assert_ok!(SubtensorModule::cancel_network_registration(
            RuntimeOrigin::signed(registrant),
            lock_id
        ));

        assert!(NetworkRegistrationQueue::<Test>::get().is_empty());
        assert!(
            pallet_balances::Locks::<Test>::get(registrant)
                .iter()
                .all(|lock| lock.id != identifier)
        );
        assert_eq!(
            pallet_balances::Pallet::<Test>::free_balance(registrant),
            free_before
        );

        // The lock is gone, so there is nothing left to release a second time.
        assert_noop!(
            SubtensorModule::cancel_network_registration(
                RuntimeOrigin::signed(registrant),
                lock_id
            ),
            Error::<Test>::NoQueuedRegistration
        );
    });
}

/// Withdrawing an offer retracts the ultimatum behind it.
///
/// The subnet it was aimed at is not evicted for declining an offer that is no longer on the table.
/// A later registration has to open its own window at its own price, which is what stops a
/// withdrawn challenge from being worth as much to the attacker as a real one.
#[test]
fn withdrawing_a_challenge_leaves_the_subnet_standing() {
    new_test_ext(0).execute_with(|| {
        let (low, high, _low_cold, registrant) = refusal_setup(4160);

        let lock_id = NetworkRegistrationLockId::<Test>::get();
        assert_ok!(register_from(registrant, 4165));
        let retracted = SubnetRefusalWindow::<Test>::get(low)
            .expect("the challenge opened a window")
            .expires_at;

        assert_ok!(SubtensorModule::cancel_network_registration(
            RuntimeOrigin::signed(registrant),
            lock_id
        ));

        // Past the deadline the retracted offer carried, so a window that survived its challenger
        // would read as lapsed here and evict.
        System::set_block_number(retracted.saturating_add(1));
        assert_ok!(register_from(registrant, 4166));

        assert!(NetworksAdded::<Test>::get(low));
        assert!(NetworksAdded::<Test>::get(high));
        assert!(DissolveCleanupQueue::<Test>::get().is_empty());
        let fresh = SubnetRefusalWindow::<Test>::get(low).expect("a fresh window at its own price");
        assert!(fresh.expires_at > retracted);
    });
}

/// A lock is only the account holder's to release, so the queue is matched on both the lock and the
/// coldkey that owns it.
#[test]
fn only_the_registrant_can_withdraw_their_registration() {
    new_test_ext(0).execute_with(|| {
        let (_low, _high, low_cold, registrant) = refusal_setup(4170);

        let lock_id = NetworkRegistrationLockId::<Test>::get();
        assert_ok!(register_from(registrant, 4175));

        assert_noop!(
            SubtensorModule::cancel_network_registration(RuntimeOrigin::signed(low_cold), lock_id),
            Error::<Test>::NoQueuedRegistration
        );
        assert_eq!(NetworkRegistrationQueue::<Test>::get().len(), 1);
    });
}

#[test]
fn prune_selection_complex_state_exhaustive() {
    new_test_ext(0).execute_with(|| {
        let imm = SubtensorModule::get_network_immunity_period();

        // ---------------------------------------------------------------------
        // Build a rich topology of networks with controlled registration times.
        // ---------------------------------------------------------------------
        // n1 + n2 in the same block (equal timestamp) to test "tie + same time".
        System::set_block_number(0);
        let n1 = add_dynamic_network(&U256::from(101), &U256::from(201));
        let n2 = add_dynamic_network(&U256::from(102), &U256::from(202)); // same registered_at as n1

        // Later registrations (strictly greater timestamp than n1/n2)
        System::set_block_number(1);
        let n3 = add_dynamic_network(&U256::from(103), &U256::from(203));

        System::set_block_number(2);
        let n4 = add_dynamic_network(&U256::from(104), &U256::from(204));

        // Create *immune* networks that will remain ineligible initially,
        // even if their price is the lowest.
        System::set_block_number(imm + 5);
        let n5 = add_dynamic_network(&U256::from(105), &U256::from(205)); // immune at first

        System::set_block_number(imm + 6);
        let n6 = add_dynamic_network(&U256::from(106), &U256::from(206)); // immune at first

        // Add 100 TAO to subnet accounts (lock)
        let subnet_account1 = SubtensorModule::get_subnet_account_id(n1).unwrap();
        let subnet_account2 = SubtensorModule::get_subnet_account_id(n2).unwrap();
        let subnet_account3 = SubtensorModule::get_subnet_account_id(n3).unwrap();
        let subnet_account4 = SubtensorModule::get_subnet_account_id(n4).unwrap();
        let subnet_account5 = SubtensorModule::get_subnet_account_id(n5).unwrap();
        let subnet_account6 = SubtensorModule::get_subnet_account_id(n6).unwrap();
        add_balance_to_coldkey_account(&subnet_account1, 100_000_000_000_u64.into());
        add_balance_to_coldkey_account(&subnet_account2, 100_000_000_000_u64.into());
        add_balance_to_coldkey_account(&subnet_account3, 100_000_000_000_u64.into());
        add_balance_to_coldkey_account(&subnet_account4, 100_000_000_000_u64.into());
        add_balance_to_coldkey_account(&subnet_account5, 100_000_000_000_u64.into());
        add_balance_to_coldkey_account(&subnet_account6, 100_000_000_000_u64.into());

        // (Root is ignored by the selector.)
        let root = NetUid::ROOT;

        // ---------------------------------------------------------------------
        // Drive pruning via the EMA/moving price used by `get_network_to_prune()`.
        // We set the moving prices directly to create deterministic selections.
        //
        // Intended prices:
        // n1: 25, n2: 25, n3: 100, n4: 1, n5: 0 (immune initially), n6: 0 (immune initially)
        // ---------------------------------------------------------------------
        SubnetMovingPrice::<Test>::insert(n1, I96F32::from_num(25));
        SubnetMovingPrice::<Test>::insert(n2, I96F32::from_num(25));
        SubnetMovingPrice::<Test>::insert(n3, I96F32::from_num(100));
        SubnetMovingPrice::<Test>::insert(n4, I96F32::from_num(1));
        SubnetMovingPrice::<Test>::insert(n5, I96F32::from_num(0));
        SubnetMovingPrice::<Test>::insert(n6, I96F32::from_num(0));

        // ---------------------------------------------------------------------
        // Phase A: Only n1..n4 are mature → lowest price (n4=1) should win.
        // ---------------------------------------------------------------------
        System::set_block_number(imm + 10);
        assert_eq!(
            SubtensorModule::get_network_to_prune(),
            Some(n4),
            "Among mature nets (n1..n4), n4 has price=1 (lowest) and should be chosen."
        );

        // ---------------------------------------------------------------------
        // Phase B: Tie on price with *same registration time* (n1 vs n2).
        // Raise n4's price to 25 so {n1=25, n2=25, n3=100, n4=25}.
        // n1 and n2 share the *same registered_at*. The tie should keep the
        // first encountered (stable iteration by key order) → n1.
        // ---------------------------------------------------------------------
        SubnetMovingPrice::<Test>::insert(n4, I96F32::from_num(25)); // n4 now 25
        assert_eq!(
            SubtensorModule::get_network_to_prune(),
            Some(n1),
            "Tie on price with equal timestamps (n1,n2) → first encountered (n1) should persist."
        );

        // ---------------------------------------------------------------------
        // Phase C: Tie on price with *different registration times*.
        // Make n3 price=25 as well. Now n1,n2,n3,n4 all have price=25.
        // Earliest registration among them is n1 (block 0).
        // ---------------------------------------------------------------------
        SubnetMovingPrice::<Test>::insert(n3, I96F32::from_num(25));
        assert_eq!(
            SubtensorModule::get_network_to_prune(),
            Some(n1),
            "Tie on price across multiple nets → earliest registration (n1) wins."
        );

        // ---------------------------------------------------------------------
        // Phase D: Immune networks ignored even if strictly cheaper (0).
        // n5 and n6 price=0 but still immune at (imm + 10). Ensure they are
        // ignored and selection remains n1.
        // ---------------------------------------------------------------------
        let now = System::block_number();
        assert!(
            now < NetworkRegisteredAt::<Test>::get(n5) + imm,
            "n5 is immune at current block"
        );
        assert!(
            now < NetworkRegisteredAt::<Test>::get(n6) + imm,
            "n6 is immune at current block"
        );
        assert_eq!(
            SubtensorModule::get_network_to_prune(),
            Some(n1),
            "Immune nets (n5,n6) must be ignored despite lower price."
        );

        // ---------------------------------------------------------------------
        // Phase E: If *all* networks are immune → return None.
        // Move clock back before any network's immunity expires.
        // ---------------------------------------------------------------------
        System::set_block_number(0);
        assert_eq!(
            SubtensorModule::get_network_to_prune(),
            None,
            "With all networks immune, there is no prunable candidate."
        );

        // ---------------------------------------------------------------------
        // Phase F: Advance beyond immunity for n5 & n6.
        // Both n5 and n6 now eligible with price=0 (lowest).
        // Tie on price; earlier registration between n5 and n6 is n5.
        // ---------------------------------------------------------------------
        System::set_block_number(2 * imm + 10);
        assert!(
            System::block_number() >= NetworkRegisteredAt::<Test>::get(n5) + imm,
            "n5 has matured"
        );
        assert!(
            System::block_number() >= NetworkRegisteredAt::<Test>::get(n6) + imm,
            "n6 has matured"
        );
        assert_eq!(
            SubtensorModule::get_network_to_prune(),
            Some(n5),
            "After immunity, n5 (price=0) should win; tie with n6 broken by earlier registration."
        );

        // ---------------------------------------------------------------------
        // Phase G: Create *sparse* netuids and ensure selection is stable.
        // Remove n5; now n6 (price=0) should be selected.
        // This validates robustness to holes / non-contiguous netuids.
        // ---------------------------------------------------------------------
        assert_ok!(SubtensorModule::do_dissolve_network(n5));
        assert_eq!(
            SubtensorModule::get_network_to_prune(),
            Some(n6),
            "After removing n5, next-lowest (n6=0) should be chosen even with sparse netuids."
        );

        // ---------------------------------------------------------------------
        // Phase H: Dynamic price changes.
        // Make n6 expensive (price 100); make n3 cheapest (price 1).
        // ---------------------------------------------------------------------
        SubnetMovingPrice::<Test>::insert(n6, I96F32::from_num(100));
        SubnetMovingPrice::<Test>::insert(n3, I96F32::from_num(1));
        assert_eq!(
            SubtensorModule::get_network_to_prune(),
            Some(n3),
            "Dynamic changes: n3 set to price=1 (lowest among eligibles) → should be pruned."
        );

        // ---------------------------------------------------------------------
        // Phase I: Tie again (n2 vs n3) but earlier registration must win.
        // Give n2 the same price as n3; n2 registered at block 0, n3 at block 1.
        // n2 should be chosen.
        // ---------------------------------------------------------------------
        SubnetMovingPrice::<Test>::insert(n2, I96F32::from_num(1));
        assert_eq!(
            SubtensorModule::get_network_to_prune(),
            Some(n2),
            "Tie on price across n2 (earlier reg) and n3 → n2 wins by timestamp."
        );

        // ---------------------------------------------------------------------
        // (Extra) Mark n2 as 'not added' to assert we honor the `added` flag,
        // then restore it to avoid side-effects on subsequent tests.
        // ---------------------------------------------------------------------
        NetworksAdded::<Test>::insert(n2, false);
        assert_ne!(
            SubtensorModule::get_network_to_prune(),
            Some(n2),
            "`added=false` must exclude n2 from consideration."
        );
        NetworksAdded::<Test>::insert(n2, true);

        // Root is always ignored even if cheapest (get_moving_alpha_price returns 1 for ROOT).
        assert_ne!(
            SubtensorModule::get_network_to_prune(),
            Some(root),
            "ROOT must never be selected for pruning."
        );
    });
}

#[test]
fn get_subnet_account_id_some_while_dissolved_cleanup_pending() {
    new_test_ext(1).execute_with(|| {
        let cold = U256::from(44_001);
        let hot = U256::from(44_002);
        let net = add_dynamic_network(&hot, &cold);
        assert_ok!(SubtensorModule::do_dissolve_network(net));
        assert!(!SubtensorModule::if_subnet_exist(net));
        assert!(DissolveCleanupQueue::<Test>::get().contains(&net));
        assert!(
            SubtensorModule::get_subnet_account_id(net).is_some(),
            "subnet TAO account must stay derivable during async dissolve cleanup"
        );
    });
}

#[test]
fn register_network_skips_dissolved_netuid() {
    new_test_ext(0).execute_with(|| {
        let dissolved = NetUid::from(1);
        DissolveCleanupQueue::<Test>::put(vec![dissolved]);

        let cold = U256::from(60);
        let hot = U256::from(61);
        let needed: u64 = SubtensorModule::get_network_lock_cost().into();
        add_balance_to_coldkey_account(&cold, needed.saturating_mul(10).into());

        assert_ok!(SubtensorModule::do_register_network(
            RuntimeOrigin::signed(cold),
            &hot,
            1,
            None,
        ));

        assert!(!NetworksAdded::<Test>::get(dissolved));
        let expected = NetUid::from(2);
        assert!(NetworksAdded::<Test>::get(expected));
        assert_eq!(SubnetOwner::<Test>::get(expected), cold);
    });
}

#[test]
fn register_network_fails_before_prune_keeps_existing() {
    new_test_ext(0).execute_with(|| {
        SubnetLimit::<Test>::put(1u16);

        let n_cold = U256::from(41);
        let n_hot = U256::from(42);
        let net = add_dynamic_network(&n_hot, &n_cold);

        let imm = SubtensorModule::get_network_immunity_period();
        System::set_block_number(imm + 50);
        Emission::<Test>::insert(net, vec![AlphaBalance::from(10)]);

        let caller_cold = U256::from(50);
        let caller_hot = U256::from(51);

        assert_err!(
            SubtensorModule::do_register_network(
                RuntimeOrigin::signed(caller_cold),
                &caller_hot,
                1,
                None,
            ),
            Error::<Test>::CannotAffordLockCost
        );

        assert!(SubtensorModule::if_subnet_exist(net));
        assert_eq!(TotalNetworks::<Test>::get(), 1);
    });
}

#[test]
fn test_migrate_network_immunity_period() {
    new_test_ext(0).execute_with(|| {
        // --------------------------------------------------------------------
        // ‼️ PRE-CONDITIONS
        // --------------------------------------------------------------------
        assert_ne!(NetworkImmunityPeriod::<Test>::get(), 864_000);
        assert!(
            !HasMigrationRun::<Test>::get(b"migrate_network_immunity_period".to_vec()),
            "HasMigrationRun should be false before migration"
        );

        // --------------------------------------------------------------------
        // ▶️  RUN MIGRATION
        // --------------------------------------------------------------------
        let weight = migrate_network_immunity_period::migrate_network_immunity_period::<Test>();

        // --------------------------------------------------------------------
        // ✅ POST-CONDITIONS
        // --------------------------------------------------------------------
        assert_eq!(
            NetworkImmunityPeriod::<Test>::get(),
            864_000,
            "NetworkImmunityPeriod should now be 864_000"
        );

        assert!(
            HasMigrationRun::<Test>::get(b"migrate_network_immunity_period".to_vec()),
            "HasMigrationRun should be true after migration"
        );

        assert!(weight != Weight::zero(), "migration weight should be > 0");
    });
}

// #[test]
// fn test_schedule_dissolve_network_execution() {
//     new_test_ext(1).execute_with(|| {
//         let block_number: u64 = 0;
//         let netuid = NetUid::from(2);
//         let tempo: u16 = 13;
//         let hotkey_account_id: U256 = U256::from(1);
//         let coldkey_account_id = U256::from(0); // Neighbour of the beast, har har
//         let (nonce, work): (u64, Vec<u8>) = SubtensorModule::create_work_for_block_number(
//             netuid,
//             block_number,
//             129123813,
//             &hotkey_account_id,
//         );

//         //add network
//         add_network(netuid, tempo, 0);

//         assert_ok!(SubtensorModule::register(
//             <<Test as Config>::RuntimeOrigin>::signed(hotkey_account_id),
//             netuid,
//             block_number,
//             nonce,
//             work.clone(),
//             hotkey_account_id,
//             coldkey_account_id
//         ));

//         assert!(SubtensorModule::if_subnet_exist(netuid));

//         assert_ok!(SubtensorModule::schedule_dissolve_network(
//             <<Test as Config>::RuntimeOrigin>::signed(coldkey_account_id),
//             netuid
//         ));

//         let current_block = System::block_number();
//         let execution_block = current_block + DissolveNetworkScheduleDuration::<Test>::get();

//         System::assert_last_event(
//             Event::DissolveNetworkScheduled {
//                 account: coldkey_account_id,
//                 netuid,
//                 execution_block,
//             }
//             .into(),
//         );

//         run_to_block(execution_block);
//         assert!(!SubtensorModule::if_subnet_exist(netuid));
//     })
// }

// #[test]
// fn test_non_owner_schedule_dissolve_network_execution() {
//     new_test_ext(1).execute_with(|| {
//         let block_number: u64 = 0;
//         let netuid = NetUid::from(2);
//         let tempo: u16 = 13;
//         let hotkey_account_id: U256 = U256::from(1);
//         let coldkey_account_id = U256::from(0); // Neighbour of the beast, har har
//         let non_network_owner_account_id = U256::from(2); //
//         let (nonce, work): (u64, Vec<u8>) = SubtensorModule::create_work_for_block_number(
//             netuid,
//             block_number,
//             129123813,
//             &hotkey_account_id,
//         );

//         //add network
//         add_network(netuid, tempo, 0);

//         assert_ok!(SubtensorModule::register(
//             <<Test as Config>::RuntimeOrigin>::signed(hotkey_account_id),
//             netuid,
//             block_number,
//             nonce,
//             work.clone(),
//             hotkey_account_id,
//             coldkey_account_id
//         ));

//         assert!(SubtensorModule::if_subnet_exist(netuid));

//         assert_ok!(SubtensorModule::schedule_dissolve_network(
//             <<Test as Config>::RuntimeOrigin>::signed(non_network_owner_account_id),
//             netuid
//         ));

//         let current_block = System::block_number();
//         let execution_block = current_block + DissolveNetworkScheduleDuration::<Test>::get();

//         System::assert_last_event(
//             Event::DissolveNetworkScheduled {
//                 account: non_network_owner_account_id,
//                 netuid,
//                 execution_block,
//             }
//             .into(),
//         );

//         run_to_block(execution_block);
//         // network exists since the caller is no the network owner
//         assert!(SubtensorModule::if_subnet_exist(netuid));
//     })
// }

// #[test]
// fn test_new_owner_schedule_dissolve_network_execution() {
//     new_test_ext(1).execute_with(|| {
//         let block_number: u64 = 0;
//         let netuid = NetUid::from(2);
//         let tempo: u16 = 13;
//         let hotkey_account_id: U256 = U256::from(1);
//         let coldkey_account_id = U256::from(0); // Neighbour of the beast, har har
//         let new_network_owner_account_id = U256::from(2); //
//         let (nonce, work): (u64, Vec<u8>) = SubtensorModule::create_work_for_block_number(
//             netuid,
//             block_number,
//             129123813,
//             &hotkey_account_id,
//         );

//         //add network
//         add_network(netuid, tempo, 0);

//         assert_ok!(SubtensorModule::register(
//             <<Test as Config>::RuntimeOrigin>::signed(hotkey_account_id),
//             netuid,
//             block_number,
//             nonce,
//             work.clone(),
//             hotkey_account_id,
//             coldkey_account_id
//         ));

//         assert!(SubtensorModule::if_subnet_exist(netuid));

//         // the account is not network owner when schedule the call
//         assert_ok!(SubtensorModule::schedule_dissolve_network(
//             <<Test as Config>::RuntimeOrigin>::signed(new_network_owner_account_id),
//             netuid
//         ));

//         let current_block = System::block_number();
//         let execution_block = current_block + DissolveNetworkScheduleDuration::<Test>::get();

//         System::assert_last_event(
//             Event::DissolveNetworkScheduled {
//                 account: new_network_owner_account_id,
//                 netuid,
//                 execution_block,
//             }
//             .into(),
//         );
//         run_to_block(current_block + 1);
//         // become network owner after call scheduled
//         crate::SubnetOwner::<Test>::insert(netuid, new_network_owner_account_id);

//         run_to_block(execution_block);
//         // network exists since the caller is no the network owner
//         assert!(!SubtensorModule::if_subnet_exist(netuid));
//     })
// }

// #[test]
// fn test_schedule_dissolve_network_execution_with_coldkey_swap() {
//     new_test_ext(1).execute_with(|| {
//         let block_number: u64 = 0;
//         let netuid = NetUid::from(2);
//         let tempo: u16 = 13;
//         let hotkey_account_id: U256 = U256::from(1);
//         let coldkey_account_id = U256::from(0); // Neighbour of the beast, har har
//         let new_network_owner_account_id = U256::from(2); //

//         add_balance_to_coldkey_account(&coldkey_account_id, 1000000000000000);

//         let (nonce, work): (u64, Vec<u8>) = SubtensorModule::create_work_for_block_number(
//             netuid,
//             block_number,
//             129123813,
//             &hotkey_account_id,
//         );

//         //add network
//         add_network(netuid, tempo, 0);

//         assert_ok!(SubtensorModule::register(
//             <<Test as Config>::RuntimeOrigin>::signed(hotkey_account_id),
//             netuid,
//             block_number,
//             nonce,
//             work.clone(),
//             hotkey_account_id,
//             coldkey_account_id
//         ));

//         assert!(SubtensorModule::if_subnet_exist(netuid));

//         // the account is not network owner when schedule the call
//         assert_ok!(SubtensorModule::schedule_swap_coldkey(
//             <<Test as Config>::RuntimeOrigin>::signed(coldkey_account_id),
//             new_network_owner_account_id
//         ));

//         let current_block = System::block_number();
//         let execution_block = current_block + ColdkeySwapScheduleDuration::<Test>::get();

//         run_to_block(execution_block - 1);

//         // the account is not network owner when schedule the call
//         assert_ok!(SubtensorModule::schedule_dissolve_network(
//             <<Test as Config>::RuntimeOrigin>::signed(new_network_owner_account_id),
//             netuid
//         ));

//         System::assert_last_event(
//             Event::DissolveNetworkScheduled {
//                 account: new_network_owner_account_id,
//                 netuid,
//                 execution_block: DissolveNetworkScheduleDuration::<Test>::get() + execution_block
//                     - 1,
//             }
//             .into(),
//         );

//         run_to_block(execution_block);
//         assert_eq!(
//             crate::SubnetOwner::<Test>::get(netuid),
//             new_network_owner_account_id
//         );

//         let current_block = System::block_number();
//         let execution_block = current_block + DissolveNetworkScheduleDuration::<Test>::get();

//         run_to_block(execution_block);
//         // network exists since the caller is no the network owner
//         assert!(!SubtensorModule::if_subnet_exist(netuid));
//     })
// }

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::networks::test_register_subnet_low_lock_cost --exact --show-output --nocapture
#[test]
fn test_register_subnet_low_lock_cost() {
    new_test_ext(1).execute_with(|| {
        NetworkMinLockCost::<Test>::set(TaoBalance::from(1_000));
        NetworkLastLockCost::<Test>::set(TaoBalance::from(1_000));

        // Make sure lock cost is lower than 100 TAO
        let lock_cost = SubtensorModule::get_network_lock_cost();
        assert!(lock_cost < 100_000_000_000_u64.into());

        let subnet_owner_coldkey = U256::from(1);
        let subnet_owner_hotkey = U256::from(2);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        assert!(SubtensorModule::if_subnet_exist(netuid));

        // Ensure that both Subnet TAO and Subnet Alpha In equal to (actual) lock_cost
        assert_eq!(SubnetTAO::<Test>::get(netuid), lock_cost);
        assert_eq!(
            SubnetAlphaIn::<Test>::get(netuid),
            lock_cost.to_u64().into()
        );
    })
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::networks::test_register_subnet_high_lock_cost --exact --show-output --nocapture
#[test]
fn test_register_subnet_high_lock_cost() {
    new_test_ext(1).execute_with(|| {
        let lock_cost = TaoBalance::from(1_000_000_000_000_u64);
        NetworkMinLockCost::<Test>::set(lock_cost);
        NetworkLastLockCost::<Test>::set(lock_cost);

        // Make sure lock cost is higher than 100 TAO
        let lock_cost = SubtensorModule::get_network_lock_cost();
        assert!(lock_cost >= 1_000_000_000_000_u64.into());

        let subnet_owner_coldkey = U256::from(1);
        let subnet_owner_hotkey = U256::from(2);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        assert!(SubtensorModule::if_subnet_exist(netuid));

        // Ensure that both Subnet TAO and Subnet Alpha In equal to 100 TAO
        assert_eq!(SubnetTAO::<Test>::get(netuid), lock_cost);
        assert_eq!(
            SubnetAlphaIn::<Test>::get(netuid),
            lock_cost.to_u64().into()
        );
    })
}

#[test]
fn test_tempo_greater_than_weight_set_rate_limit() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_hotkey = U256::from(1);
        let subnet_owner_coldkey = U256::from(2);

        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);

        // Get tempo
        let tempo = SubtensorModule::get_tempo(netuid);

        let weights_set_rate_limit = SubtensorModule::get_weights_set_rate_limit(netuid);

        assert!(tempo as u64 >= weights_set_rate_limit);
    })
}

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

#[test]
fn dissolve_clears_all_mechanism_scoped_maps_for_all_mechanisms() {
    new_test_ext(0).execute_with(|| {
        // Create a subnet we can dissolve.
        let owner_cold = U256::from(123);
        let owner_hot = U256::from(456);
        let net = add_dynamic_network(&owner_hot, &owner_cold);

        // Add 100 TAO to subnet account (lock)
        let subnet_account = SubtensorModule::get_subnet_account_id(net).unwrap();
        add_balance_to_coldkey_account(&subnet_account, 100_000_000_000_u64.into());

        // We'll use two mechanisms for this subnet.
        MechanismCountCurrent::<Test>::insert(net, MechId::from(2));
        let m0 = MechId::from(0u8);
        let m1 = MechId::from(1u8);

        let idx0 = SubtensorModule::get_mechanism_storage_index(net, m0);
        let idx1 = SubtensorModule::get_mechanism_storage_index(net, m1);

        // Minimal content to ensure each storage actually has keys for BOTH mechanisms.

        // --- Weights (DMAP: (netuid_index, uid) -> Vec<(dest_uid, weight_u16)>)
        Weights::<Test>::insert(idx0, 0u16, vec![(1u16, 1u16)]);
        Weights::<Test>::insert(idx1, 0u16, vec![(2u16, 1u16)]);

        // --- Bonds (DMAP: (netuid_index, uid) -> Vec<(dest_uid, weight_u16)>)
        Bonds::<Test>::insert(idx0, 0u16, vec![(1u16, 1u16)]);
        Bonds::<Test>::insert(idx1, 0u16, vec![(2u16, 1u16)]);

        // --- TimelockedWeightCommits (DMAP: (netuid_index, epoch) -> VecDeque<...>)
        let hotkey = U256::from(1);
        TimelockedWeightCommits::<Test>::insert(
            idx0,
            1u64,
            VecDeque::from([(hotkey, 1u64, Default::default(), Default::default())]),
        );
        TimelockedWeightCommits::<Test>::insert(
            idx1,
            2u64,
            VecDeque::from([(hotkey, 2u64, Default::default(), Default::default())]),
        );

        // --- Incentive (MAP: netuid_index -> Vec<u16>)
        Incentive::<Test>::insert(idx0, vec![PerU16::from_parts(1), PerU16::from_parts(2)]);
        Incentive::<Test>::insert(idx1, vec![PerU16::from_parts(3), PerU16::from_parts(4)]);

        // --- LastUpdate (MAP: netuid_index -> Vec<u64>)
        LastUpdate::<Test>::insert(idx0, vec![42u64]);
        LastUpdate::<Test>::insert(idx1, vec![84u64]);

        // Sanity: keys are present before dissolve.
        assert!(Weights::<Test>::contains_key(idx0, 0u16));
        assert!(Weights::<Test>::contains_key(idx1, 0u16));
        assert!(Bonds::<Test>::contains_key(idx0, 0u16));
        assert!(Bonds::<Test>::contains_key(idx1, 0u16));
        assert!(TimelockedWeightCommits::<Test>::contains_key(idx0, 1u64));
        assert!(TimelockedWeightCommits::<Test>::contains_key(idx1, 2u64));
        assert!(Incentive::<Test>::contains_key(idx0));
        assert!(Incentive::<Test>::contains_key(idx1));
        assert!(LastUpdate::<Test>::contains_key(idx0));
        assert!(LastUpdate::<Test>::contains_key(idx1));
        assert!(MechanismCountCurrent::<Test>::contains_key(net));

        // --- Dissolve the subnet ---
        assert_ok!(SubtensorModule::do_dissolve_network(net));
        run_block_idle();

        // After dissolve, ALL mechanism-scoped items must be cleared for ALL mechanisms.

        // Weights/Bonds double-maps should have no entries under either index.
        assert!(Weights::<Test>::iter_prefix(idx0).next().is_none());
        assert!(Weights::<Test>::iter_prefix(idx1).next().is_none());
        assert!(Bonds::<Test>::iter_prefix(idx0).next().is_none());
        assert!(Bonds::<Test>::iter_prefix(idx1).next().is_none());

        // WeightCommits (OptionQuery) should have no keys remaining.
        assert!(WeightCommits::<Test>::iter_prefix(idx0).next().is_none());
        assert!(WeightCommits::<Test>::iter_prefix(idx1).next().is_none());
        assert!(!WeightCommits::<Test>::contains_key(idx0, owner_hot));
        assert!(!WeightCommits::<Test>::contains_key(idx1, owner_cold));

        // TimelockedWeightCommits (ValueQuery) — ensure both prefix spaces empty and keys gone.
        assert!(
            TimelockedWeightCommits::<Test>::iter_prefix(idx0)
                .next()
                .is_none()
        );
        assert!(
            TimelockedWeightCommits::<Test>::iter_prefix(idx1)
                .next()
                .is_none()
        );
        assert!(!TimelockedWeightCommits::<Test>::contains_key(idx0, 1u64));
        assert!(!TimelockedWeightCommits::<Test>::contains_key(idx1, 2u64));

        // Single-map per-mechanism vectors cleared.
        assert!(!Incentive::<Test>::contains_key(idx0));
        assert!(!Incentive::<Test>::contains_key(idx1));
        assert!(!LastUpdate::<Test>::contains_key(idx0));
        assert!(!LastUpdate::<Test>::contains_key(idx1));

        // MechanismCountCurrent cleared
        assert!(!MechanismCountCurrent::<Test>::contains_key(net));
    });
}

#[test]
fn dissolve_clears_all_lock_maps_for_removed_network() {
    new_test_ext(0).execute_with(|| {
        // Create a subnet we can dissolve.
        let owner_cold = U256::from(123);
        let owner_hot = U256::from(456);
        let net = add_dynamic_network(&owner_hot, &owner_cold);

        // Add TAO to subnet account so dissolve can proceed.
        let subnet_account = SubtensorModule::get_subnet_account_id(net).unwrap();
        add_balance_to_coldkey_account(&subnet_account, 100_000_000_000_u64.into());

        // Non-owner coldkeys / hotkeys.
        let cold_1 = U256::from(1001);
        let cold_2 = U256::from(1002);
        let hot_1 = U256::from(2001);
        let hot_2 = U256::from(2002);

        // Another subnet to ensure dissolve only clears `net`.
        let other_net = NetUid::from(u16::from(net) + 1);

        // Explicit LockState initialization
        let lock_a = LockState {
            locked_mass: 10u64.into(),
            conviction: U64F64::from_num(1.5),
            last_update: 1,
        };

        let lock_b = LockState {
            locked_mass: 20u64.into(),
            conviction: U64F64::from_num(2.5),
            last_update: 2,
        };

        // --- Lock: (coldkey, netuid, hotkey)
        Lock::<Test>::insert((cold_1, net, hot_1), lock_a.clone());
        LockingColdkeys::<Test>::insert((net, hot_1, cold_1), ());
        Lock::<Test>::insert((cold_2, net, hot_2), lock_b.clone());
        LockingColdkeys::<Test>::insert((net, hot_2, cold_2), ());

        // Same cold/hot on another net should survive.
        Lock::<Test>::insert((cold_1, other_net, hot_1), lock_a.clone());
        LockingColdkeys::<Test>::insert((other_net, hot_1, cold_1), ());

        // --- HotkeyLock
        HotkeyLock::<Test>::insert(net, hot_1, lock_a.clone());
        HotkeyLock::<Test>::insert(net, hot_2, lock_b.clone());
        HotkeyLock::<Test>::insert(other_net, hot_1, lock_a.clone());

        // --- DecayingHotkeyLock
        DecayingHotkeyLock::<Test>::insert(net, hot_1, lock_a.clone());
        DecayingHotkeyLock::<Test>::insert(net, hot_2, lock_b.clone());
        DecayingHotkeyLock::<Test>::insert(other_net, hot_1, lock_a.clone());

        // --- OwnerLock
        OwnerLock::<Test>::insert(net, lock_a.clone());
        OwnerLock::<Test>::insert(other_net, lock_b.clone());

        // --- DecayingLock
        DecayingLock::<Test>::insert(cold_1, net, false);
        DecayingLock::<Test>::insert(cold_2, net, false);
        DecayingLock::<Test>::insert(cold_1, other_net, false);

        // Sanity checks before dissolve
        assert!(Lock::<Test>::contains_key((cold_1, net, hot_1)));
        assert!(Lock::<Test>::contains_key((cold_2, net, hot_2)));
        assert!(LockingColdkeys::<Test>::contains_key((net, hot_1, cold_1)));
        assert!(LockingColdkeys::<Test>::contains_key((net, hot_2, cold_2)));

        assert!(HotkeyLock::<Test>::contains_key(net, hot_1));
        assert!(HotkeyLock::<Test>::contains_key(net, hot_2));

        assert!(DecayingHotkeyLock::<Test>::contains_key(net, hot_1));
        assert!(DecayingHotkeyLock::<Test>::contains_key(net, hot_2));

        assert!(OwnerLock::<Test>::contains_key(net));

        assert!(DecayingLock::<Test>::contains_key(cold_1, net));
        assert!(DecayingLock::<Test>::contains_key(cold_2, net));

        // Sanity: other net keys are present before dissolve.
        assert!(Lock::<Test>::contains_key((cold_1, other_net, hot_1)));
        assert!(LockingColdkeys::<Test>::contains_key((
            other_net, hot_1, cold_1
        )));
        assert!(HotkeyLock::<Test>::contains_key(other_net, hot_1));
        assert!(DecayingHotkeyLock::<Test>::contains_key(other_net, hot_1));
        assert!(OwnerLock::<Test>::contains_key(other_net));
        assert!(DecayingLock::<Test>::contains_key(cold_1, other_net));

        // --- Dissolve ---
        assert_ok!(SubtensorModule::do_dissolve_network(net));
        run_block_idle();

        // Ensure removed
        assert!(!Lock::<Test>::contains_key((cold_1, net, hot_1)));
        assert!(!Lock::<Test>::contains_key((cold_2, net, hot_2)));
        assert!(!LockingColdkeys::<Test>::contains_key((net, hot_1, cold_1)));
        assert!(!LockingColdkeys::<Test>::contains_key((net, hot_2, cold_2)));

        assert!(!HotkeyLock::<Test>::contains_key(net, hot_1));
        assert!(!HotkeyLock::<Test>::contains_key(net, hot_2));
        assert!(HotkeyLock::<Test>::iter_prefix(net).next().is_none());

        assert!(!DecayingHotkeyLock::<Test>::contains_key(net, hot_1));
        assert!(!DecayingHotkeyLock::<Test>::contains_key(net, hot_2));
        assert!(
            DecayingHotkeyLock::<Test>::iter_prefix(net)
                .next()
                .is_none()
        );

        assert!(!OwnerLock::<Test>::contains_key(net));

        assert!(!DecayingLock::<Test>::contains_key(cold_1, net));
        assert!(!DecayingLock::<Test>::contains_key(cold_2, net));

        // Ensure other_net is untouched
        assert!(Lock::<Test>::contains_key((cold_1, other_net, hot_1)));
        assert!(LockingColdkeys::<Test>::contains_key((
            other_net, hot_1, cold_1
        )));
        assert!(HotkeyLock::<Test>::contains_key(other_net, hot_1));
        assert!(DecayingHotkeyLock::<Test>::contains_key(other_net, hot_1));
        assert!(OwnerLock::<Test>::contains_key(other_net));
        assert!(DecayingLock::<Test>::contains_key(cold_1, other_net));
    });
}

fn owner_alpha_from_lock_and_price(lock_cost_u64: u64, price: U64F64) -> u64 {
    let alpha = (U64F64::from_num(lock_cost_u64)
        .checked_div(price)
        .unwrap_or_default())
    .floor();

    if alpha > U64F64::from_num(u64::MAX) {
        u64::MAX
    } else {
        alpha.to_num::<u64>()
    }
}

#[test]
fn median_subnet_alpha_price_returns_one_when_no_eligible_subnet_prices() {
    new_test_ext(0).execute_with(|| {
        let one = U64F64::from_num(1u64);

        // Empty state.
        assert_eq!(SubtensorModule::get_median_subnet_alpha_price(), one);

        // ROOT must be ignored.
        NetworksAdded::<Test>::insert(NetUid::ROOT, true);
        assert_eq!(SubtensorModule::get_median_subnet_alpha_price(), one);

        // Zero-priced subnet must be ignored.
        let zero_cold = U256::from(101);
        let zero_hot = U256::from(102);
        let zero_netuid = add_dynamic_network(&zero_hot, &zero_cold);
        setup_reserves(zero_netuid, TaoBalance::ZERO, AlphaBalance::from(100u64));
        assert_eq!(
            <Test as pallet::Config>::SwapInterface::current_alpha_price(zero_netuid.into()),
            U64F64::from_num(0u64)
        );
        assert_eq!(SubtensorModule::get_median_subnet_alpha_price(), one);

        // added=false subnet must be ignored as well.
        let hidden_cold = U256::from(103);
        let hidden_hot = U256::from(104);
        let hidden_netuid = add_dynamic_network(&hidden_hot, &hidden_cold);
        setup_reserves(
            hidden_netuid,
            TaoBalance::from(900u64),
            AlphaBalance::from(100u64),
        );
        NetworksAdded::<Test>::insert(hidden_netuid, false);

        assert_eq!(SubtensorModule::get_median_subnet_alpha_price(), one);
    });
}

#[test]
fn median_subnet_alpha_price_returns_middle_value_for_odd_unsorted_prices() {
    new_test_ext(0).execute_with(|| {
        let n1 = add_dynamic_network(&U256::from(201), &U256::from(200));
        let n2 = add_dynamic_network(&U256::from(203), &U256::from(202));
        let n3 = add_dynamic_network(&U256::from(205), &U256::from(204));

        // Unsorted prices: 7, 2, 5 -> median should be 5.
        setup_reserves(n1, TaoBalance::from(700u64), AlphaBalance::from(100u64));
        setup_reserves(n2, TaoBalance::from(200u64), AlphaBalance::from(100u64));
        setup_reserves(n3, TaoBalance::from(500u64), AlphaBalance::from(100u64));

        assert_eq!(
            <Test as pallet::Config>::SwapInterface::current_alpha_price(n1.into()),
            U96F32::from_num(7u64)
        );
        assert_eq!(
            <Test as pallet::Config>::SwapInterface::current_alpha_price(n2.into()),
            U96F32::from_num(2u64)
        );
        assert_eq!(
            <Test as pallet::Config>::SwapInterface::current_alpha_price(n3.into()),
            U96F32::from_num(5u64)
        );

        assert_eq!(
            SubtensorModule::get_median_subnet_alpha_price(),
            U96F32::from_num(5u64)
        );
    });
}

#[test]
fn median_subnet_alpha_price_averages_even_prices_and_ignores_root_zero_and_unadded() {
    new_test_ext(0).execute_with(|| {
        // If ROOT were included, its price would be 1 and change the median.
        NetworksAdded::<Test>::insert(NetUid::ROOT, true);

        let n1 = add_dynamic_network(&U256::from(301), &U256::from(300)); // eligible, price 2
        let n2 = add_dynamic_network(&U256::from(303), &U256::from(302)); // hidden,   price 4
        let n3 = add_dynamic_network(&U256::from(305), &U256::from(304)); // eligible, price 8
        let n4 = add_dynamic_network(&U256::from(307), &U256::from(306)); // zero,     price 0

        setup_reserves(n1, TaoBalance::from(200u64), AlphaBalance::from(100u64));
        setup_reserves(n2, TaoBalance::from(400u64), AlphaBalance::from(100u64));
        setup_reserves(n3, TaoBalance::from(800u64), AlphaBalance::from(100u64));
        setup_reserves(n4, TaoBalance::ZERO, AlphaBalance::from(100u64));

        NetworksAdded::<Test>::insert(n2, false);

        assert_eq!(
            <Test as pallet::Config>::SwapInterface::current_alpha_price(n1.into()),
            U96F32::from_num(2u64)
        );
        assert_eq!(
            <Test as pallet::Config>::SwapInterface::current_alpha_price(n2.into()),
            U96F32::from_num(4u64)
        );
        assert_eq!(
            <Test as pallet::Config>::SwapInterface::current_alpha_price(n3.into()),
            U96F32::from_num(8u64)
        );
        assert_eq!(
            <Test as pallet::Config>::SwapInterface::current_alpha_price(n4.into()),
            U96F32::from_num(0u64)
        );

        // Eligible prices are only {2, 8}, so the median is (2 + 8) / 2 = 5.
        assert_eq!(
            SubtensorModule::get_median_subnet_alpha_price(),
            U96F32::from_num(5u64)
        );
    });
}

#[test]
fn register_network_seeds_first_subnet_from_fallback_price_one_and_keeps_lock_in_pool() {
    new_test_ext(1).execute_with(|| {
        let new_cold = U256::from(1001);
        let new_hot = U256::from(1002);
        let new_netuid = SubtensorModule::get_next_netuid();

        let lock_cost_u64: u64 = SubtensorModule::get_network_lock_cost().into();
        let pre_registration_median = SubtensorModule::get_median_subnet_alpha_price();

        let pool_initial_tao = SubtensorModule::get_network_min_lock();
        let pool_initial_tao_u64 = pool_initial_tao.to_u64();
        let total_pool_tao_u64 = lock_cost_u64.max(pool_initial_tao_u64);
        let owner_alpha_tao_equivalent_u64 =
            total_pool_tao_u64.saturating_sub(pool_initial_tao_u64);

        let expected_pool_alpha_u64 =
            owner_alpha_from_lock_and_price(total_pool_tao_u64, pre_registration_median);
        let expected_pool_alpha: AlphaBalance = expected_pool_alpha_u64.into();

        let expected_owner_alpha_u64 = owner_alpha_from_lock_and_price(
            owner_alpha_tao_equivalent_u64,
            pre_registration_median,
        );
        let expected_owner_alpha: AlphaBalance = expected_owner_alpha_u64.into();

        let expected_alpha_issuance: AlphaBalance = expected_pool_alpha_u64
            .saturating_add(expected_owner_alpha_u64)
            .into();

        let expected_recycled: TaoBalance = lock_cost_u64.saturating_sub(total_pool_tao_u64).into();

        assert_eq!(pre_registration_median, U96F32::from_num(1u64));
        assert_eq!(expected_pool_alpha_u64, total_pool_tao_u64);
        assert_eq!(expected_owner_alpha_u64, owner_alpha_tao_equivalent_u64);
        assert_eq!(expected_recycled, TaoBalance::ZERO);

        add_balance_to_coldkey_account(&new_cold, lock_cost_u64.saturating_mul(2).into());

        assert_ok!(SubtensorModule::do_register_network(
            RuntimeOrigin::signed(new_cold),
            &new_hot,
            1,
            None,
        ));

        assert!(SubtensorModule::if_subnet_exist(new_netuid));
        assert_eq!(TotalNetworks::<Test>::get(), 1);
        assert_eq!(SubnetOwner::<Test>::get(new_netuid), new_cold);
        assert_eq!(SubnetOwnerHotkey::<Test>::get(new_netuid), new_hot);
        assert_eq!(
            SubtensorModule::get_subnet_locked_balance(new_netuid),
            TaoBalance::from(lock_cost_u64)
        );

        assert_eq!(
            SubnetTAO::<Test>::get(new_netuid),
            TaoBalance::from(total_pool_tao_u64)
        );
        assert_eq!(SubnetAlphaIn::<Test>::get(new_netuid), expected_pool_alpha);
        assert_eq!(
            SubnetAlphaOut::<Test>::get(new_netuid),
            expected_owner_alpha
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &new_hot, &new_cold, new_netuid,
            ),
            expected_owner_alpha
        );
        assert_eq!(
            TotalHotkeyAlpha::<Test>::get(new_hot, new_netuid),
            expected_owner_alpha
        );
        assert_eq!(
            SubtensorModule::get_alpha_issuance(new_netuid),
            expected_alpha_issuance
        );
        assert_eq!(
            RAORecycledForRegistration::<Test>::get(new_netuid),
            expected_recycled
        );

        assert_eq!(
            <Test as pallet::Config>::SwapInterface::current_alpha_price(new_netuid.into()),
            U96F32::from_num(1u64)
        );

        System::assert_last_event(Event::NetworkAdded(new_netuid, 1).into());
    });
}

#[test]
fn register_network_seeds_new_subnet_from_even_median_snapshot() {
    new_test_ext(0).execute_with(|| {
        let n1 = add_dynamic_network(&U256::from(1201), &U256::from(1200));
        let n2 = add_dynamic_network(&U256::from(1203), &U256::from(1202));

        // Existing prices are {5, 2} -> pre-registration median is 3.5.
        setup_reserves(n1, TaoBalance::from(500u64), AlphaBalance::from(100u64));
        setup_reserves(n2, TaoBalance::from(200u64), AlphaBalance::from(100u64));

        let pre_registration_median = SubtensorModule::get_median_subnet_alpha_price();
        assert_eq!(pre_registration_median, U96F32::from_num(3.5));

        let new_cold = U256::from(1300);
        let new_hot = U256::from(1301);
        let new_netuid = SubtensorModule::get_next_netuid();

        let lock_cost_u64: u64 = SubtensorModule::get_network_lock_cost().into();
        let pool_initial_tao_u64 = SubtensorModule::get_network_min_lock().to_u64();
        let total_pool_tao_u64 = lock_cost_u64.max(pool_initial_tao_u64);
        let owner_alpha_tao_equivalent_u64 =
            total_pool_tao_u64.saturating_sub(pool_initial_tao_u64);

        let expected_pool_alpha_u64 =
            owner_alpha_from_lock_and_price(total_pool_tao_u64, pre_registration_median);
        let expected_pool_alpha: AlphaBalance = expected_pool_alpha_u64.into();

        let expected_owner_alpha_u64 = owner_alpha_from_lock_and_price(
            owner_alpha_tao_equivalent_u64,
            pre_registration_median,
        );
        let expected_owner_alpha: AlphaBalance = expected_owner_alpha_u64.into();

        add_balance_to_coldkey_account(&new_cold, lock_cost_u64.saturating_mul(2).into());

        assert_ok!(SubtensorModule::do_register_network(
            RuntimeOrigin::signed(new_cold),
            &new_hot,
            1,
            None,
        ));

        let new_subnet_price =
            <Test as pallet::Config>::SwapInterface::current_alpha_price(new_netuid.into());
        let post_registration_median = SubtensorModule::get_median_subnet_alpha_price();

        assert!(SubtensorModule::if_subnet_exist(new_netuid));
        assert_eq!(SubnetOwner::<Test>::get(new_netuid), new_cold);
        assert_eq!(SubnetOwnerHotkey::<Test>::get(new_netuid), new_hot);
        assert_eq!(
            SubtensorModule::get_subnet_locked_balance(new_netuid),
            TaoBalance::from(lock_cost_u64)
        );

        assert_eq!(
            SubnetTAO::<Test>::get(new_netuid),
            TaoBalance::from(total_pool_tao_u64)
        );
        assert_eq!(SubnetAlphaIn::<Test>::get(new_netuid), expected_pool_alpha);
        assert_eq!(
            SubnetAlphaOut::<Test>::get(new_netuid),
            expected_owner_alpha
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &new_hot, &new_cold, new_netuid,
            ),
            expected_owner_alpha
        );
        assert_eq!(
            TotalHotkeyAlpha::<Test>::get(new_hot, new_netuid),
            expected_owner_alpha
        );

        // The new subnet is seeded from the pre-registration median snapshot,
        // so it is no longer initialized at the old 1:1 seed price.
        assert_ne!(new_subnet_price, U96F32::from_num(1u64));
        assert!(new_subnet_price >= pre_registration_median);

        // With prices {2, seeded_price, 5}, the live median becomes the new subnet price.
        assert_eq!(post_registration_median, new_subnet_price);

        // A 1:1 seed would have alpha_in == tao_in, which should not happen here.
        let wrong_price_one_pool_alpha: AlphaBalance = total_pool_tao_u64.into();
        assert_ne!(
            SubnetAlphaIn::<Test>::get(new_netuid),
            wrong_price_one_pool_alpha
        );
    });
}

#[test]
fn register_network_fails_without_balance_and_does_not_write_owner_alpha_state() {
    new_test_ext(0).execute_with(|| {
        let cold = U256::from(2001);
        let hot = U256::from(2002);
        let would_be_netuid = SubtensorModule::get_next_netuid();

        assert_eq!(
            SubtensorModule::get_coldkey_balance(&cold),
            TaoBalance::ZERO
        );

        assert_err!(
            SubtensorModule::do_register_network(RuntimeOrigin::signed(cold), &hot, 1, None,),
            Error::<Test>::CannotAffordLockCost
        );

        assert!(!SubtensorModule::if_subnet_exist(would_be_netuid));
        assert_eq!(TotalNetworks::<Test>::get(), 0);
        assert_eq!(
            SubnetAlphaIn::<Test>::get(would_be_netuid),
            AlphaBalance::ZERO
        );
        assert_eq!(
            SubnetAlphaOut::<Test>::get(would_be_netuid),
            AlphaBalance::ZERO
        );
        assert_eq!(
            SubtensorModule::get_subnet_locked_balance(would_be_netuid),
            TaoBalance::ZERO
        );
        assert_eq!(
            RAORecycledForRegistration::<Test>::get(would_be_netuid),
            TaoBalance::ZERO
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &hot,
                &cold,
                would_be_netuid,
            ),
            AlphaBalance::ZERO
        );
    });
}

#[test]
fn register_network_non_associated_hotkey_does_not_withdraw_or_write_owner_alpha_state() {
    new_test_ext(0).execute_with(|| {
        let original_cold = U256::from(3001);
        let shared_hot = U256::from(3002);
        let existing_netuid = add_dynamic_network(&shared_hot, &original_cold);

        let original_stake_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &shared_hot,
            &original_cold,
            existing_netuid,
        );
        let original_alpha_out_before = SubnetAlphaOut::<Test>::get(existing_netuid);

        let attacker_cold = U256::from(3003);
        let would_be_netuid = SubtensorModule::get_next_netuid();
        let lock_cost_u64: u64 = SubtensorModule::get_network_lock_cost().into();

        add_balance_to_coldkey_account(&attacker_cold, lock_cost_u64.into());
        let attacker_balance_before = SubtensorModule::get_coldkey_balance(&attacker_cold);

        assert_err!(
            SubtensorModule::do_register_network(
                RuntimeOrigin::signed(attacker_cold),
                &shared_hot,
                1,
                None,
            ),
            Error::<Test>::NonAssociatedColdKey
        );

        // Attacker was not charged.
        assert_eq!(
            SubtensorModule::get_coldkey_balance(&attacker_cold),
            attacker_balance_before
        );

        // Existing owner state is untouched.
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &shared_hot,
                &original_cold,
                existing_netuid,
            ),
            original_stake_before
        );
        assert_eq!(
            SubnetAlphaOut::<Test>::get(existing_netuid),
            original_alpha_out_before
        );
        assert_eq!(SubnetOwner::<Test>::get(existing_netuid), original_cold);

        // No new subnet / owner-alpha state was written.
        assert!(!SubtensorModule::if_subnet_exist(would_be_netuid));
        assert_eq!(TotalNetworks::<Test>::get(), 1);
        assert_eq!(
            SubnetAlphaOut::<Test>::get(would_be_netuid),
            AlphaBalance::ZERO
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &shared_hot,
                &attacker_cold,
                would_be_netuid,
            ),
            AlphaBalance::ZERO
        );
    });
}

#[test]
fn registered_subnet_counter_bumps_on_first_registration() {
    new_test_ext(1).execute_with(|| {
        let cold = U256::from(1);
        let hot = U256::from(2);

        let netuid = add_dynamic_network(&hot, &cold);

        assert_eq!(
            SubtensorModule::get_registered_subnet_counter(netuid),
            1,
            "first registration of a netuid must leave counter == 1"
        );
    });
}

#[test]
fn registered_subnet_counter_is_independent_per_netuid() {
    new_test_ext(1).execute_with(|| {
        let n1 = add_dynamic_network(&U256::from(10), &U256::from(11));
        let n2 = add_dynamic_network(&U256::from(20), &U256::from(21));

        assert_ne!(n1, n2);
        assert_eq!(SubtensorModule::get_registered_subnet_counter(n1), 1);
        assert_eq!(SubtensorModule::get_registered_subnet_counter(n2), 1);
    });
}

#[test]
fn registered_subnet_counter_survives_dissolve_and_bumps_on_reregistration() {
    new_test_ext(1).execute_with(|| {
        // Force reuse of the same netuid on re-registration by pinning the
        // active subnet cap so the next registration must prune.
        SubtensorModule::set_max_subnets(2);

        let owner_cold = U256::from(100);
        let owner_hot = U256::from(101);
        let netuid = add_dynamic_network(&owner_hot, &owner_cold);
        assert_eq!(SubtensorModule::get_registered_subnet_counter(netuid), 1);

        // Dissolve: counter is intentionally *not* cleared — stale consumers
        // can still detect the pre-dereg lifetime if they stored the counter
        // value they observed at approval time.
        assert_ok!(SubtensorModule::do_dissolve_network(netuid));
        run_block_idle();
        assert!(!SubtensorModule::if_subnet_exist(netuid));
        assert_eq!(
            SubtensorModule::get_registered_subnet_counter(netuid),
            1,
            "dissolve must not clear or reset the counter"
        );

        // Re-register. With the cap pinned, the prune selector reuses the
        // freed netuid; the counter bumps to 2 so that any state still keyed
        // to the prior value becomes unreachable under the new registration.
        let reg_netuid = add_dynamic_network(&owner_hot, &owner_cold);
        assert_eq!(
            reg_netuid, netuid,
            "the pruned netuid should be reused under the subnet cap"
        );
        assert_eq!(
            SubtensorModule::get_registered_subnet_counter(netuid),
            2,
            "re-registration must bump counter"
        );
    });
}

#[test]
fn dissolve_async_cleanup_leaves_phase_unset_until_idle_finishes() {
    new_test_ext(0).execute_with(|| {
        let owner_cold = U256::from(910);
        let owner_hot = U256::from(911);
        let net = add_dynamic_network(&owner_hot, &owner_cold);

        assert_ok!(SubtensorModule::do_dissolve_network(net));
        assert!(
            DissolveCleanupQueue::<Test>::get().contains(&net),
            "dissolved netuid should be queued for on_idle cleanup"
        );
        assert!(
            CurrentDissolveCleanupStatus::<Test>::get().is_none(),
            "global cleanup phase is only driven from on_idle (not from do_dissolve_network)"
        );

        run_block_idle();

        assert!(
            !DissolveCleanupQueue::<Test>::get().contains(&net),
            "idle cleanup should drain the dissolved net from the queue"
        );
        assert!(
            CurrentDissolveCleanupStatus::<Test>::get().is_none(),
            "when the queue is empty, global cleanup phase storage must be cleared"
        );
    });
}

#[test]
fn dissolve_full_on_idle_emits_dissolved_network_data_cleaned_and_clears_phase() {
    // `frame_system::Pallet::events()` stays empty at block #0 in the test externalities;
    // use a non-zero block like other event-asserting tests (`recycle_alpha`, etc.).
    new_test_ext(1).execute_with(|| {
        let owner_cold = U256::from(930);
        let owner_hot = U256::from(931);
        let net = add_dynamic_network(&owner_hot, &owner_cold);

        assert_ok!(SubtensorModule::do_dissolve_network(net));
        System::reset_events();
        run_block_idle();

        assert!(
            System::events().iter().any(|e| {
                matches!(
                    &e.event,
                    RuntimeEvent::SubtensorModule(Event::NetworkDissolveCleanupCompleted { netuid: n })
                        if *n == net
                )
            }),
            "expected NetworkDissolveCleanupCompleted after async dissolve pipeline"
        );
        assert!(
            CurrentDissolveCleanupStatus::<Test>::get().is_none(),
            "global cleanup phase storage must be cleared when the queue is empty"
        );
    });
}

#[test]
fn dissolve_two_networks_fifo_cleanup_drains_queue() {
    new_test_ext(0).execute_with(|| {
        let n1 = add_dynamic_network(&U256::from(940), &U256::from(941));
        let n2 = add_dynamic_network(&U256::from(942), &U256::from(943));

        assert_ok!(SubtensorModule::do_dissolve_network(n1));
        assert_ok!(SubtensorModule::do_dissolve_network(n2));
        assert_eq!(DissolveCleanupQueue::<Test>::get(), vec![n1, n2]);

        let mut guard = 0u32;
        while !DissolveCleanupQueue::<Test>::get().is_empty() {
            guard = guard.saturating_add(1);
            assert!(
                guard < 256,
                "dissolve cleanup should drain in finite idle passes (guard={guard})"
            );
            run_block_idle();
        }

        assert!(!SubtensorModule::if_subnet_exist(n1));
        assert!(!SubtensorModule::if_subnet_exist(n2));
        assert!(
            CurrentDissolveCleanupStatus::<Test>::get().is_none(),
            "no stale phase after queue drain"
        );
    });
}

#[test]
fn set_new_network_state_registers_subnet_with_expected_state() {
    new_test_ext(1).execute_with(|| {
        let cold = U256::from(9001);
        let hot = U256::from(9002);
        let lock_amount = SubtensorModule::get_network_lock_cost();
        add_balance_to_coldkey_account(&cold, lock_amount.saturating_mul(2.into()).into());
        TotalIssuance::<Test>::mutate(|total| *total = total.saturating_add(lock_amount));

        let median_price = SubtensorModule::get_median_subnet_alpha_price();
        let netuid = SubtensorModule::get_next_netuid();

        assert_ok!(SubtensorModule::set_new_network_state(
            &cold,
            &hot,
            1,
            None,
            lock_amount,
            median_price,
            None,
        ));

        assert!(SubtensorModule::if_subnet_exist(netuid));
        assert_eq!(SubnetOwner::<Test>::get(netuid), cold);
        assert_eq!(SubnetMechanism::<Test>::get(netuid), 1);
        assert_eq!(SubnetLocked::<Test>::get(netuid), lock_amount);
        assert_eq!(
            SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hot),
            Ok(0)
        );
    });
}

#[test]
fn register_network_queues_when_waiting_for_dissolve_cleanup() {
    new_test_ext(0).execute_with(|| {
        SubnetLimit::<Test>::put(2u16);

        let n1 = add_dynamic_network(&U256::from(9102), &U256::from(9101));
        let _n2 = add_dynamic_network(&U256::from(9202), &U256::from(9201));

        assert_ok!(SubtensorModule::do_dissolve_network(n1));
        assert!(DissolveCleanupQueue::<Test>::get().contains(&n1));

        let cold = U256::from(9301);
        let hot = U256::from(9302);
        let lock_amount = SubtensorModule::get_network_lock_cost();
        add_balance_to_coldkey_account(&cold, lock_amount.saturating_mul(2.into()).into());
        TotalIssuance::<Test>::mutate(|total| *total = total.saturating_add(lock_amount));

        assert_ok!(SubtensorModule::do_register_network(
            RuntimeOrigin::signed(cold),
            &hot,
            1,
            None,
        ));

        assert_eq!(NetworkRegistrationQueue::<Test>::get().len(), 1);
        assert_eq!(NetworkRegistrationQueue::<Test>::get()[0].coldkey, cold);
        assert_eq!(TotalNetworks::<Test>::get(), 1);
        assert!(!SubtensorModule::hotkey_account_exists(&hot));
    });
}

#[test]
fn process_network_registration_queue_registers_after_cleanup_slot_available() {
    new_test_ext(0).execute_with(|| {
        SubnetLimit::<Test>::put(2u16);

        let n1 = add_dynamic_network(&U256::from(9402), &U256::from(9401));
        let n2 = add_dynamic_network(&U256::from(9502), &U256::from(9501));

        assert_ok!(SubtensorModule::do_dissolve_network(n1));
        assert!(DissolveCleanupQueue::<Test>::get().contains(&n1));

        let cold = U256::from(9601);
        let hot = U256::from(9602);
        let lock_amount = SubtensorModule::get_network_lock_cost();
        add_balance_to_coldkey_account(&cold, lock_amount.saturating_mul(3.into()).into());
        TotalIssuance::<Test>::mutate(|total| *total = total.saturating_add(lock_amount));

        assert_ok!(SubtensorModule::do_register_network(
            RuntimeOrigin::signed(cold),
            &hot,
            1,
            None,
        ));
        assert_eq!(NetworkRegistrationQueue::<Test>::get().len(), 1);

        // Simulate dissolve cleanup completing and freeing a subnet slot.
        DissolveCleanupQueue::<Test>::kill();

        run_network_registration_queue();

        assert!(NetworkRegistrationQueue::<Test>::get().is_empty());
        assert!(SubtensorModule::hotkey_account_exists(&hot));
        assert_eq!(TotalNetworks::<Test>::get(), 2);

        let registered_netuid = NetworksAdded::<Test>::iter()
            .find(|(netuid, added)| *added && *netuid != n2)
            .map(|(netuid, _)| netuid)
            .expect("queued registration should create a new subnet");
        assert_eq!(SubnetOwner::<Test>::get(registered_netuid), cold);
    });
}

#[test]
fn register_network_prune_registers_registration_queued() {
    new_test_ext(0).execute_with(|| {
        SubnetLimit::<Test>::put(2u16);

        let n1 = add_dynamic_network(&U256::from(9702), &U256::from(9701));
        let n2 = add_dynamic_network(&U256::from(9802), &U256::from(9801));

        let imm = SubtensorModule::get_network_immunity_period();
        System::set_block_number(imm + 100);
        Emission::<Test>::insert(n1, vec![AlphaBalance::from(1)]);
        Emission::<Test>::insert(n2, vec![AlphaBalance::from(1_000)]);

        let cold = U256::from(9901);
        let hot = U256::from(9902);
        let lock_amount = SubtensorModule::get_network_lock_cost();
        add_balance_to_coldkey_account(&cold, lock_amount.saturating_mul(10.into()).into());
        TotalIssuance::<Test>::mutate(|total| *total = total.saturating_add(lock_amount));

        assert_ok!(SubtensorModule::do_register_network(
            RuntimeOrigin::signed(cold),
            &hot,
            1,
            None,
        ));

        assert!(NetworkRegistrationQueue::<Test>::get().len() == 1);

        // The first registration to reach the cap opens the owner's window rather than pruning,
        // so n1 is still here and the challenger waits.
        let expires_at = SubnetRefusalWindow::<Test>::get(n1)
            .expect("window should open")
            .expires_at;
        assert!(!DissolveCleanupQueue::<Test>::get().contains(&n1));
        assert!(NetworksAdded::<Test>::get(n1));

        // Unanswered, so the next registration takes the slot.
        System::set_block_number(expires_at.saturating_add(1));
        let cold2 = U256::from(9903);
        add_balance_to_coldkey_account(&cold2, lock_amount.saturating_mul(10.into()).into());
        TotalIssuance::<Test>::mutate(|total| *total = total.saturating_add(lock_amount));
        assert_ok!(register_from(cold2, 9904));

        assert!(DissolveCleanupQueue::<Test>::get().contains(&n1));
        assert!(!NetworksAdded::<Test>::get(n1));
    });
}

#[test]
fn set_new_network_state_fails_when_subnet_limit_reached() {
    new_test_ext(1).execute_with(|| {
        SubnetLimit::<Test>::put(1u16);
        let _n1 = add_dynamic_network(&U256::from(10_002), &U256::from(10_001));

        let cold = U256::from(10_011);
        let hot = U256::from(10_012);
        let lock_amount = SubtensorModule::get_network_lock_cost();
        add_balance_to_coldkey_account(&cold, lock_amount.saturating_mul(2.into()).into());

        assert_err!(
            SubtensorModule::set_new_network_state(
                &cold,
                &hot,
                1,
                None,
                lock_amount,
                SubtensorModule::get_median_subnet_alpha_price(),
                None,
            ),
            Error::<Test>::SubnetLimitReached
        );

        // No partial state was written.
        assert_eq!(TotalNetworks::<Test>::get(), 1);
        assert!(!SubtensorModule::hotkey_account_exists(&hot));
    });
}

#[test]
fn set_new_network_state_stores_identity_and_emits_events() {
    new_test_ext(1).execute_with(|| {
        let cold = U256::from(10_101);
        let hot = U256::from(10_102);
        let lock_amount = SubtensorModule::get_network_lock_cost();
        add_balance_to_coldkey_account(&cold, lock_amount.saturating_mul(2.into()).into());

        let identity = SubnetIdentityOfV3 {
            subnet_name: b"my subnet".to_vec(),
            github_repo: b"https://github.com/example/repo".to_vec(),
            subnet_contact: b"contact@example.com".to_vec(),
            subnet_url: b"https://example.com".to_vec(),
            discord: b"discord".to_vec(),
            description: b"description".to_vec(),
            logo_url: b"https://example.com/logo.png".to_vec(),
            additional: b"".to_vec(),
        };

        let netuid = SubtensorModule::get_next_netuid();
        System::reset_events();

        assert_ok!(SubtensorModule::set_new_network_state(
            &cold,
            &hot,
            1,
            Some(identity.clone()),
            lock_amount,
            SubtensorModule::get_median_subnet_alpha_price(),
            None,
        ));

        assert_eq!(SubnetIdentitiesV3::<Test>::get(netuid), Some(identity));
        let events = System::events();
        assert!(events.iter().any(|e| matches!(
            &e.event,
            RuntimeEvent::SubtensorModule(Event::SubnetIdentitySet(n)) if *n == netuid
        )));
        assert!(events.iter().any(|e| matches!(
            &e.event,
            RuntimeEvent::SubtensorModule(Event::NetworkAdded(n, m)) if *n == netuid && *m == 1
        )));
    });
}

#[test]
fn set_new_network_state_uses_provided_median_price_for_pool_alpha() {
    new_test_ext(1).execute_with(|| {
        let cold = U256::from(10_201);
        let hot = U256::from(10_202);

        // Lock twice the min lock so the pool is seeded from the actual lock amount.
        let min_lock = SubtensorModule::get_network_min_lock();
        let lock_amount = min_lock.saturating_mul(2.into());
        add_balance_to_coldkey_account(&cold, lock_amount.saturating_mul(2.into()).into());

        let netuid = SubtensorModule::get_next_netuid();
        let price = U64F64::from_num(2);

        assert_ok!(SubtensorModule::set_new_network_state(
            &cold,
            &hot,
            1,
            None,
            lock_amount,
            price,
            None,
        ));

        // Pool TAO equals the actual lock; alpha reserve is tao / price.
        assert_eq!(SubnetTAO::<Test>::get(netuid), lock_amount);
        let expected_alpha: u64 = u64::from(lock_amount) / 2;
        assert_eq!(
            SubnetAlphaIn::<Test>::get(netuid),
            AlphaBalance::from(expected_alpha)
        );
    });
}

#[test]
fn set_new_network_state_seeds_pool_with_min_lock_floor() {
    new_test_ext(1).execute_with(|| {
        let cold = U256::from(10_301);
        let hot = U256::from(10_302);
        add_balance_to_coldkey_account(&cold, 1_000_000_000.into());

        let netuid = SubtensorModule::get_next_netuid();
        let min_lock = SubtensorModule::get_network_min_lock();

        // Zero lock: the pool must still be seeded with the min lock floor.
        assert_ok!(SubtensorModule::set_new_network_state(
            &cold,
            &hot,
            1,
            None,
            TaoBalance::ZERO,
            U64F64::from_num(1),
            None,
        ));

        assert_eq!(SubnetTAO::<Test>::get(netuid), min_lock);
        assert_eq!(
            SubnetAlphaIn::<Test>::get(netuid),
            AlphaBalance::from(u64::from(min_lock))
        );
        assert_eq!(SubnetLocked::<Test>::get(netuid), TaoBalance::ZERO);
    });
}

#[test]
fn set_new_network_state_fund_locked_releases_balance_lock() {
    new_test_ext(1).execute_with(|| {
        let cold = U256::from(10_401);
        let hot = U256::from(10_402);
        let lock_amount = SubtensorModule::get_network_lock_cost();
        add_balance_to_coldkey_account(&cold, lock_amount.saturating_mul(2.into()).into());

        let lock_id = NetworkRegistrationLockId::<Test>::get();
        let mut identifier = [0u8; 8];
        identifier[..4].copy_from_slice(b"rglk");
        identifier[4..8].copy_from_slice(&lock_id.to_le_bytes());

        assert_ok!(SubtensorModule::lock_network_registration_cost(
            &cold,
            lock_amount.into(),
            0
        ));
        assert!(
            pallet_balances::Locks::<Test>::get(cold)
                .iter()
                .any(|l| l.id == identifier),
            "registration lock must exist before processing"
        );

        let netuid = SubtensorModule::get_next_netuid();

        assert_ok!(SubtensorModule::set_new_network_state(
            &cold,
            &hot,
            1,
            None,
            lock_amount,
            SubtensorModule::get_median_subnet_alpha_price(),
            Some(lock_id),
        ));

        assert!(
            pallet_balances::Locks::<Test>::get(cold)
                .iter()
                .all(|l| l.id != identifier),
            "registration lock must be released after processing"
        );
        assert!(SubtensorModule::if_subnet_exist(netuid));
        assert_eq!(SubnetLocked::<Test>::get(netuid), lock_amount);
    });
}

#[test]
fn process_network_registration_queue_noop_when_empty() {
    new_test_ext(1).execute_with(|| {
        let networks_before = TotalNetworks::<Test>::get();

        SubtensorModule::process_network_registration_queue();

        assert!(NetworkRegistrationQueue::<Test>::get().is_empty());
        assert_eq!(TotalNetworks::<Test>::get(), networks_before);
    });
}

#[test]
fn process_network_registration_queue_waits_for_cleanup_completion() {
    new_test_ext(0).execute_with(|| {
        SubnetLimit::<Test>::put(2u16);

        let n1 = add_dynamic_network(&U256::from(10_502), &U256::from(10_501));
        let _n2 = add_dynamic_network(&U256::from(10_602), &U256::from(10_601));

        assert_ok!(SubtensorModule::do_dissolve_network(n1));

        let cold = U256::from(10_701);
        let hot = U256::from(10_702);
        let lock_amount = SubtensorModule::get_network_lock_cost();
        add_balance_to_coldkey_account(&cold, lock_amount.saturating_mul(2.into()).into());

        assert_ok!(SubtensorModule::do_register_network(
            RuntimeOrigin::signed(cold),
            &hot,
            1,
            None,
        ));
        assert_eq!(NetworkRegistrationQueue::<Test>::get().len(), 1);

        // Cleanup is still pending: the queued registration must not be released.
        SubtensorModule::process_network_registration_queue();

        assert_eq!(NetworkRegistrationQueue::<Test>::get().len(), 1);
        assert!(!SubtensorModule::hotkey_account_exists(&hot));
        assert_eq!(TotalNetworks::<Test>::get(), 1);

        // Once cleanup completes, the same call releases the registration.
        DissolveCleanupQueue::<Test>::kill();
        SubtensorModule::process_network_registration_queue();

        assert!(NetworkRegistrationQueue::<Test>::get().is_empty());
        assert!(SubtensorModule::hotkey_account_exists(&hot));
        assert_eq!(TotalNetworks::<Test>::get(), 2);
    });
}

#[test]
fn process_network_registration_queue_processes_one_entry_per_call() {
    new_test_ext(0).execute_with(|| {
        SubnetLimit::<Test>::put(3u16);

        let n1 = add_dynamic_network(&U256::from(10_802), &U256::from(10_801));
        let n2 = add_dynamic_network(&U256::from(10_902), &U256::from(10_901));
        let _n3 = add_dynamic_network(&U256::from(11_002), &U256::from(11_001));

        assert_ok!(SubtensorModule::do_dissolve_network(n1));
        assert_ok!(SubtensorModule::do_dissolve_network(n2));
        assert_eq!(DissolveCleanupQueue::<Test>::get().len(), 2);

        let cold_a = U256::from(11_101);
        let hot_a = U256::from(11_102);
        let cold_b = U256::from(11_201);
        let hot_b = U256::from(11_202);
        for cold in [&cold_a, &cold_b] {
            let lock_amount = SubtensorModule::get_network_lock_cost();
            add_balance_to_coldkey_account(cold, lock_amount.saturating_mul(2.into()).into());
        }

        assert_ok!(SubtensorModule::do_register_network(
            RuntimeOrigin::signed(cold_a),
            &hot_a,
            1,
            None,
        ));
        assert_ok!(SubtensorModule::do_register_network(
            RuntimeOrigin::signed(cold_b),
            &hot_b,
            1,
            None,
        ));
        assert_eq!(NetworkRegistrationQueue::<Test>::get().len(), 2);

        DissolveCleanupQueue::<Test>::kill();

        // First call processes only the first (FIFO) entry.
        SubtensorModule::process_network_registration_queue();
        assert_eq!(NetworkRegistrationQueue::<Test>::get().len(), 1);
        assert!(SubtensorModule::hotkey_account_exists(&hot_a));
        assert!(!SubtensorModule::hotkey_account_exists(&hot_b));
        assert_eq!(NetworkRegistrationQueue::<Test>::get()[0].coldkey, cold_b);

        // Second call processes the remaining entry.
        SubtensorModule::process_network_registration_queue();
        assert!(NetworkRegistrationQueue::<Test>::get().is_empty());
        assert!(SubtensorModule::hotkey_account_exists(&hot_b));
        assert_eq!(TotalNetworks::<Test>::get(), 3);
    });
}

#[test]
fn process_network_registration_queue_unlocks_funds_and_charges_coldkey() {
    new_test_ext(0).execute_with(|| {
        SubnetLimit::<Test>::put(2u16);

        let n1 = add_dynamic_network(&U256::from(11_302), &U256::from(11_301));
        let n2 = add_dynamic_network(&U256::from(11_402), &U256::from(11_401));

        assert_ok!(SubtensorModule::do_dissolve_network(n1));

        let cold = U256::from(11_501);
        let hot = U256::from(11_502);
        let lock_amount = SubtensorModule::get_network_lock_cost();
        let lock_id = NetworkRegistrationLockId::<Test>::get();
        let mut identifier = [0u8; 8];
        identifier[..4].copy_from_slice(b"rglk");
        identifier[4..8].copy_from_slice(&lock_id.to_le_bytes());
        add_balance_to_coldkey_account(&cold, lock_amount.saturating_mul(3.into()).into());

        assert_ok!(SubtensorModule::do_register_network(
            RuntimeOrigin::signed(cold),
            &hot,
            1,
            None,
        ));

        // Funds are locked while queued.
        assert!(
            pallet_balances::Locks::<Test>::get(cold)
                .iter()
                .any(|l| l.id == identifier)
        );
        let queued_lock = NetworkRegistrationQueue::<Test>::get()[0].lock_amount;
        // Use free balance: the reducible balance is already reduced by the lock.
        let balance_before = pallet_balances::Pallet::<Test>::free_balance(cold);

        DissolveCleanupQueue::<Test>::kill();
        SubtensorModule::process_network_registration_queue();

        // Lock released and the lock cost transferred to the new subnet.
        assert!(
            pallet_balances::Locks::<Test>::get(cold)
                .iter()
                .all(|l| l.id != identifier)
        );
        let balance_after = pallet_balances::Pallet::<Test>::free_balance(cold);
        assert_eq!(balance_before.saturating_sub(balance_after), queued_lock);

        let new_netuid = NetworksAdded::<Test>::iter()
            .find(|(netuid, added)| *added && *netuid != n2)
            .map(|(netuid, _)| netuid)
            .expect("queued registration should create a new subnet");
        assert_eq!(SubnetOwner::<Test>::get(new_netuid), cold);
        assert_eq!(SubnetLocked::<Test>::get(new_netuid), queued_lock);
    });
}

/// Queue depth is not a function of elapsed time.
///
/// Immunity bought by answering a challenge does lapse, and the subnet then returns to the
/// eviction pool and can be challenged again. Were a matched challenger left queued, one subnet
/// would add an entry every cycle forever, and nothing in the eviction pool would bound a list
/// that every registration on the chain decodes. Dequeueing on match is what stops that.
#[test]
fn matched_challengers_do_not_accumulate_across_immunity_cycles() {
    new_test_ext(0).execute_with(|| {
        let (low, _high, low_cold, registrant) = refusal_setup(4200);

        let big = SubtensorModule::get_network_lock_cost().saturating_mul(10_000.into());
        add_balance_to_coldkey_account(&low_cold, big.into());
        add_balance_to_coldkey_account(&registrant, big.into());
        TotalIssuance::<Test>::mutate(|t| *t = t.saturating_add(big.saturating_mul(2.into())));

        // Two subnets exist, so the evictable pool is never larger than two.
        for cycle in 0..4u64 {
            assert_eq!(SubtensorModule::get_network_to_prune(), Some(low));
            assert_ok!(register_from(registrant, 4300u64.saturating_add(cycle)));
            assert_ok!(SubtensorModule::exercise_first_refusal(
                RuntimeOrigin::signed(low_cold),
                low
            ));
            assert!(
                NetworkRegistrationQueue::<Test>::get().is_empty(),
                "cycle {cycle}: the answered challenger was refunded and dequeued"
            );
            System::set_block_number(NetworkImmuneUntil::<Test>::get(low).saturating_add(1));
        }
    });
}

/// The answered path is bounded. The unanswered path is not, and this pins the difference.
///
/// A challenger opens a window and is queued, but nothing is pruned yet. Nobody answers. The next
/// registration to arrive clears the lapsed window, prunes, and joins the queue behind that
/// challenger. Two entries in, one slot out. Depth climbs by one per unanswered cycle, and every
/// registration decodes the queue, so this is a real cost rather than a cosmetic one. Stated in
/// the PR body rather than fixed: closing it means handing a lapsed window's challenger the slot
/// without waiting for a second registration to trigger the prune.
#[test]
fn an_unanswered_cycle_queues_two_and_frees_one() {
    new_test_ext(0).execute_with(|| {
        let (low, _high, _low_cold, registrant) = refusal_setup(4500);

        let big = SubtensorModule::get_network_lock_cost().saturating_mul(10_000.into());
        add_balance_to_coldkey_account(&registrant, big.into());
        TotalIssuance::<Test>::mutate(|t| *t = t.saturating_add(big));

        assert_ok!(register_from(registrant, 4510));
        assert!(SubnetRefusalWindow::<Test>::contains_key(low));
        assert_eq!(NetworkRegistrationQueue::<Test>::get().len(), 1);

        let expires = SubnetRefusalWindow::<Test>::get(low).unwrap().expires_at;
        System::set_block_number(expires.saturating_add(1));

        assert_ok!(register_from(registrant, 4511));
        assert_eq!(
            NetworkRegistrationQueue::<Test>::get().len(),
            2,
            "the lapsed window's challenger is still queued when the pruner joins"
        );

        DissolveCleanupQueue::<Test>::kill();
        run_network_registration_queue();
        assert_eq!(
            NetworkRegistrationQueue::<Test>::get().len(),
            1,
            "one entry outlives the cycle that created it"
        );
    });
}

/// One open window diverts the next registration rather than holding up the chain.
///
/// `get_network_to_prune` passes over a subnet whose owner is still inside the deadline, so the
/// scan names the next candidate instead. Refusing here rather than skipping would have let a
/// single lock freeze every at-cap registration, and cancelling refunds in full, so the same lock
/// could be recycled indefinitely. Skipping prices the attack properly: freezing the chain now
/// takes a live window on every evictable subnet at once, each backed by its own full lock.
#[test]
fn a_live_window_diverts_the_next_registration_instead_of_blocking_it() {
    new_test_ext(0).execute_with(|| {
        let (low, high, _low_cold, registrant) = refusal_setup(4600);

        let big = SubtensorModule::get_network_lock_cost().saturating_mul(10_000.into());
        add_balance_to_coldkey_account(&registrant, big.into());
        TotalIssuance::<Test>::mutate(|t| *t = t.saturating_add(big));

        assert_ok!(register_from(registrant, 4610));
        assert!(SubnetRefusalWindow::<Test>::contains_key(low));

        // Not refused with `SubnetChallengeInProgress`: the scan skips `low` and names `high`.
        assert_ok!(register_from(registrant, 4611));
        assert!(
            SubnetRefusalWindow::<Test>::contains_key(high),
            "the second challenge landed on the next candidate"
        );
        assert!(
            SubnetRefusalWindow::<Test>::contains_key(low),
            "and the first window is untouched, so its owner keeps the full deadline"
        );
    });
}

/// Guards the benchmark fixture: a window with no challenger behind it does not prune.
///
/// `refusal_state` tests challenger presence before expiry, so a window whose `challenger_lock_id`
/// is absent from `NetworkRegistrationQueue` resolves to `Unchallenged` rather than `Lapsed`, and
/// the registration opens a fresh window instead of pruning. `register_network_pruning` used to
/// build exactly that state and so measured the wrong branch, and its old
/// `assert!(!queue.is_empty())` passed on both, which is why it went unnoticed. `fill_subnets` now
/// queues the matching entry; this pins the behavior that made the omission matter.
#[test]
fn a_window_with_no_queued_challenger_opens_afresh_instead_of_pruning() {
    new_test_ext(0).execute_with(|| {
        let (low, _high, _low_cold, registrant) = refusal_setup(4400);

        // exactly what benchmarks::helpers::lapsed_refusal_window() writes
        SubnetRefusalWindow::<Test>::insert(
            low,
            RefusalWindow {
                offer: TaoBalance::from(0u64),
                expires_at: 0,
                challenger_lock_id: 0,
                immunity_period: 0,
            },
        );
        assert!(NetworkRegistrationQueue::<Test>::get().is_empty());

        assert_ok!(register_from(registrant, 4500));

        // the benchmark's assertion -- passes either way
        assert!(!NetworkRegistrationQueue::<Test>::get().is_empty());

        // but nothing was pruned
        assert!(
            DissolveCleanupQueue::<Test>::get().is_empty(),
            "no teardown was started, so the prune branch never ran"
        );
        let fresh = SubnetRefusalWindow::<Test>::get(low).expect("a fresh window was opened");
        assert!(
            fresh.expires_at > 0,
            "the fixture's lapsed window (expires_at 0) was replaced by a live one, \
             so the registration opened a window rather than pruning"
        );
    });
}

/// Documents pre-existing base behavior. This PR neither introduces it nor relies on it, and the
/// test is here because the collateral behind a queued registration is what an owner is deciding
/// against. Reported separately rather than patched here.
///
/// Registration collateral uses `LockableCurrency::set_lock` under a per-lock-id identifier, and
/// Substrate locks overlap rather than stack. `lock_network_registration_cost` guards with
/// `can_remove_balance_from_coldkey_account`, which nets out already-frozen funds, so a coldkey
/// cannot open a second lock for its whole balance. It CAN open two locks of unequal size: the
/// pair freezes max(a, b) while promising a + b, leaving the difference spendable.
#[test]
fn unequal_registration_locks_freeze_max_not_sum() {
    new_test_ext(0).execute_with(|| {
        let coldkey = U256::from(9001u64);
        let big: u64 = 100_000_000_000;
        let small: u64 = 50_000_000_000;
        add_balance_to_coldkey_account(&coldkey, big.saturating_add(small).into());

        assert_ok!(SubtensorModule::lock_network_registration_cost(
            &coldkey,
            big.into(),
            0
        ));
        // The guard rejects a second lock for the full balance -- overlap is not freely exploitable.
        assert_err!(
            SubtensorModule::lock_network_registration_cost(&coldkey, big.into(), 1),
            Error::<Test>::InsufficientTaoBalance
        );
        // But an unequal second lock fits inside what the first one left reducible.
        assert_ok!(SubtensorModule::lock_network_registration_cost(
            &coldkey,
            small.into(),
            1
        ));

        let free: u64 = Balances::free_balance(coldkey).into();
        let usable: u64 = Balances::usable_balance(coldkey).into();
        let frozen = free.saturating_sub(usable);

        assert_eq!(
            frozen,
            big,
            "two locks promising {} freeze only max(a, b) = {}",
            big.saturating_add(small),
            big
        );
        assert_eq!(
            usable, small,
            "the {small} difference stays spendable while still promised to a queued offer"
        );
    });
}
