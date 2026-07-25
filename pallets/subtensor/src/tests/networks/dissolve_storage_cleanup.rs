#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::unwrap_used)]
//! Dissolve clears per-subnet, mechanism-scoped, and lock map storage.

use super::prelude::*;

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
        assert!(!SubtensorModule::subnet_exists(net));
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
