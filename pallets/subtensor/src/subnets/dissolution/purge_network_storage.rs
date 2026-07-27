//! Weight-metered deletion of per-`netuid` storage during dissolve cleanup.
//!
//! Each helper returns `false` / incomplete when the [`WeightMeter`] cannot
//! cover another read or write so the phased cleanup can resume next block.

use super::*;
use frame_support::weights::WeightMeter;
use subtensor_runtime_common::{NetUid, NetUidStorageIndex, clear_prefix_with_meter};

impl<T: Config> Pallet<T> {
    /// Clear map-backed subnet storage (keys, axons, mechanism weights, leases, …).
    pub fn remove_network_map_parameters(netuid: NetUid, weight_meter: &mut WeightMeter) -> bool {
        let write_weight = T::DbWeight::get().writes(1);

        let result = clear_prefix_with_meter(weight_meter, write_weight, |limit| {
            Keys::<T>::clear_prefix(netuid, limit, None)
        }) && clear_prefix_with_meter(weight_meter, write_weight, |limit| {
            Uids::<T>::clear_prefix(netuid, limit, None)
        }) && clear_prefix_with_meter(weight_meter, write_weight, |limit| {
            BlockAtRegistration::<T>::clear_prefix(netuid, limit, None)
        }) && clear_prefix_with_meter(weight_meter, write_weight, |limit| {
            Axons::<T>::clear_prefix(netuid, limit, None)
        }) && clear_prefix_with_meter(weight_meter, write_weight, |limit| {
            NeuronCertificates::<T>::clear_prefix(netuid, limit, None)
        }) && clear_prefix_with_meter(weight_meter, write_weight, |limit| {
            Prometheus::<T>::clear_prefix(netuid, limit, None)
        }) && clear_prefix_with_meter(weight_meter, write_weight, |limit| {
            AlphaDividendsPerSubnet::<T>::clear_prefix(netuid, limit, None)
        }) && clear_prefix_with_meter(weight_meter, write_weight, |limit| {
            PendingChildKeys::<T>::clear_prefix(netuid, limit, None)
        }) && clear_prefix_with_meter(weight_meter, write_weight, |limit| {
            AssociatedEvmAddress::<T>::clear_prefix(netuid, limit, None)
        }) && clear_prefix_with_meter(weight_meter, write_weight, |limit| {
            AssociatedUidsByEvmAddress::<T>::clear_prefix(netuid, limit, None)
        }) && clear_prefix_with_meter(weight_meter, write_weight, |limit| {
            HotkeyLock::<T>::clear_prefix(netuid, limit, None)
        }) && clear_prefix_with_meter(weight_meter, write_weight, |limit| {
            DecayingHotkeyLock::<T>::clear_prefix(netuid, limit, None)
        }) && clear_prefix_with_meter(weight_meter, write_weight, |limit| {
            LockingColdkeys::<T>::clear_prefix((netuid,), limit, None)
        }) && clear_prefix_with_meter(weight_meter, write_weight, |limit| {
            // Lock metadata only. Alpha (including collateral stake) was already
            // pro-rata converted to coldkey free TAO in AlphaInOutStakesSettleStakes;
            // unlocking here would double-pay. Clearing drops the now-meaningless
            // MinerCollateral rows for the dissolved netuid.
            MinerCollateral::<T>::clear_prefix((netuid,), limit, None)
        }) && clear_prefix_with_meter(weight_meter, write_weight, |limit| {
            ColdkeyMinerCollateral::<T>::clear_prefix(netuid, limit, None)
        }) && clear_prefix_with_meter(weight_meter, write_weight, |limit| {
            ColdkeyCollateralHotkeys::<T>::clear_prefix(netuid, limit, None)
        });

        if !result {
            return false;
        }

        let read_weight = T::DbWeight::get().reads(1);
        if !weight_meter.can_consume(read_weight) {
            return false;
        }
        weight_meter.consume(read_weight);
        let mechanisms: u8 = MechanismCountCurrent::<T>::get(netuid).into();

        for subid in 0..mechanisms {
            let mechanism_weight = T::DbWeight::get().reads_writes(1, 2);
            if !weight_meter.can_consume(mechanism_weight) {
                return false;
            }
            weight_meter.consume(mechanism_weight);
            let netuid_index = Self::get_mechanism_storage_index(netuid, subid.into());

            LastUpdate::<T>::remove(netuid_index);
            Incentive::<T>::remove(netuid_index);

            let result = clear_prefix_with_meter(weight_meter, write_weight, |limit| {
                WeightCommits::<T>::clear_prefix(netuid_index, limit, None)
            }) && clear_prefix_with_meter(weight_meter, write_weight, |limit| {
                TimelockedWeightCommits::<T>::clear_prefix(netuid_index, limit, None)
            }) && clear_prefix_with_meter(weight_meter, write_weight, |limit| {
                CRV3WeightCommits::<T>::clear_prefix(netuid_index, limit, None)
            }) && clear_prefix_with_meter(weight_meter, write_weight, |limit| {
                CRV3WeightCommitsV2::<T>::clear_prefix(netuid_index, limit, None)
            }) && clear_prefix_with_meter(weight_meter, write_weight, |limit| {
                Bonds::<T>::clear_prefix(netuid_index, limit, None)
            }) && clear_prefix_with_meter(weight_meter, write_weight, |limit| {
                Weights::<T>::clear_prefix(netuid_index, limit, None)
            });

            if !result {
                return false;
            }
        }

        let removal_weight = T::DbWeight::get().writes(3);
        if !weight_meter.can_consume(removal_weight) {
            return false;
        }
        weight_meter.consume(removal_weight);
        RevealPeriodEpochs::<T>::remove(netuid);
        MechanismCountCurrent::<T>::remove(netuid);
        MechanismEmissionSplit::<T>::remove(netuid);

        if !clear_prefix_with_meter(weight_meter, write_weight, |limit| {
            LastHotkeySwapOnNetuid::<T>::clear_prefix(netuid, limit, None)
        }) || !clear_prefix_with_meter(weight_meter, write_weight, |limit| {
            HotkeySuccessor::<T>::clear_prefix(netuid, limit, None)
        }) || !clear_prefix_with_meter(weight_meter, write_weight, |limit| {
            HotkeyRoot::<T>::clear_prefix(netuid, limit, None)
        }) {
            return false;
        }

        if let Some(lease_id) = SubnetUidToLeaseId::<T>::get(netuid) {
            if !clear_prefix_with_meter(weight_meter, write_weight, |limit| {
                SubnetLeaseShares::<T>::clear_prefix(lease_id, limit, None)
            }) {
                return false;
            }
            let lease_weight = T::DbWeight::get().writes(3);
            if !weight_meter.can_consume(lease_weight) {
                return false;
            }
            weight_meter.consume(lease_weight);
            SubnetLeases::<T>::remove(lease_id);
            AccumulatedLeaseDividends::<T>::remove(lease_id);
            SubnetUidToLeaseId::<T>::remove(netuid);
        }

        true
    }

    /// Remove scalar per-netuid hyperparams and identity; returns false if weight insufficient.
    pub fn remove_network_parameters(netuid: NetUid, weight_meter: &mut WeightMeter) -> bool {
        // Flat write charge for the `::remove(netuid)` list below. Bump this when
        // adding or removing entries from that list so the weight stays in step.
        let removal_weight = T::DbWeight::get().writes(82);
        if !weight_meter.can_consume(removal_weight) {
            return false;
        }
        weight_meter.consume(removal_weight);
        SubnetOwner::<T>::remove(netuid);
        SubnetworkN::<T>::remove(netuid);
        NetworkRegisteredAt::<T>::remove(netuid);
        Active::<T>::remove(netuid);
        Emission::<T>::remove(netuid);
        Consensus::<T>::remove(netuid);
        Dividends::<T>::remove(netuid);
        ValidatorPermit::<T>::remove(netuid);
        ValidatorTrust::<T>::remove(netuid);
        Tempo::<T>::remove(netuid);
        Kappa::<T>::remove(netuid);
        Difficulty::<T>::remove(netuid);
        MaxAllowedUids::<T>::remove(netuid);
        ImmunityPeriod::<T>::remove(netuid);
        ActivityCutoff::<T>::remove(netuid);
        MinAllowedWeights::<T>::remove(netuid);
        RegistrationsThisInterval::<T>::remove(netuid);
        POWRegistrationsThisInterval::<T>::remove(netuid);
        BurnRegistrationsThisInterval::<T>::remove(netuid);
        SubnetAlphaInEmission::<T>::remove(netuid);
        SubnetAlphaOutEmission::<T>::remove(netuid);
        SubnetTaoInEmission::<T>::remove(netuid);
        SubnetVolume::<T>::remove(netuid);
        SubnetMovingPrice::<T>::remove(netuid);
        SubnetTaoFlow::<T>::remove(netuid);
        SubnetEmaTaoFlow::<T>::remove(netuid);
        SubnetProtocolFlow::<T>::remove(netuid);
        SubnetEmaProtocolFlow::<T>::remove(netuid);
        SubnetExcessTao::<T>::remove(netuid);
        SubnetRootSellTao::<T>::remove(netuid);
        TokenSymbol::<T>::remove(netuid);
        SubnetMechanism::<T>::remove(netuid);
        SubnetOwnerHotkey::<T>::remove(netuid);
        NetworkRegistrationAllowed::<T>::remove(netuid);
        NetworkPowRegistrationAllowed::<T>::remove(netuid);
        TransferToggle::<T>::remove(netuid);
        SubnetLocked::<T>::remove(netuid);
        LargestLocked::<T>::remove(netuid);
        FirstEmissionBlockNumber::<T>::remove(netuid);
        PendingValidatorEmission::<T>::remove(netuid);
        PendingServerEmission::<T>::remove(netuid);
        PendingRootAlphaDivs::<T>::remove(netuid);
        PendingOwnerCut::<T>::remove(netuid);
        MinerBurned::<T>::remove(netuid);
        BlocksSinceLastStep::<T>::remove(netuid);
        LastMechansimStepBlock::<T>::remove(netuid);
        LastAdjustmentBlock::<T>::remove(netuid);
        ServingRateLimit::<T>::remove(netuid);
        Rho::<T>::remove(netuid);
        AlphaSigmoidSteepness::<T>::remove(netuid);
        MaxAllowedValidators::<T>::remove(netuid);
        BondsMovingAverage::<T>::remove(netuid);
        BondsPenalty::<T>::remove(netuid);
        BondsResetOn::<T>::remove(netuid);
        WeightsSetRateLimit::<T>::remove(netuid);
        ValidatorPruneLen::<T>::remove(netuid);
        ScalingLawPower::<T>::remove(netuid);
        TargetRegistrationsPerInterval::<T>::remove(netuid);
        CommitRevealWeightsEnabled::<T>::remove(netuid);
        BurnHalfLife::<T>::remove(netuid);
        BurnIncreaseMult::<T>::remove(netuid);
        CollateralLockShare::<T>::remove(netuid);
        CollateralDrainRatio::<T>::remove(netuid);
        Burn::<T>::remove(netuid);
        MinBurn::<T>::remove(netuid);
        MaxBurn::<T>::remove(netuid);
        MinDifficulty::<T>::remove(netuid);
        MaxDifficulty::<T>::remove(netuid);
        RegistrationsThisBlock::<T>::remove(netuid);
        EMAPriceHalvingBlocks::<T>::remove(netuid);
        RAORecycledForRegistration::<T>::remove(netuid);
        MaxRegistrationsPerBlock::<T>::remove(netuid);
        WeightsVersionKey::<T>::remove(netuid);
        LiquidAlphaOn::<T>::remove(netuid);
        Yuma3On::<T>::remove(netuid);
        AlphaValues::<T>::remove(netuid);
        SubtokenEnabled::<T>::remove(netuid);
        OwnerCutAutoLockEnabled::<T>::remove(netuid);
        ImmuneOwnerUidsLimit::<T>::remove(netuid);
        StakeWeight::<T>::remove(netuid);
        LoadedEmission::<T>::remove(netuid);
        OwnerLock::<T>::remove(netuid);
        DecayingOwnerLock::<T>::remove(netuid);
        ActivityCutoffFactorMilli::<T>::remove(netuid);
        LastEpochBlock::<T>::remove(netuid);
        PendingEpochAt::<T>::remove(netuid);
        SubnetEpochIndex::<T>::remove(netuid);

        if SubnetIdentitiesV3::<T>::contains_key(netuid) {
            SubnetIdentitiesV3::<T>::remove(netuid);
            Self::deposit_event(Event::SubnetIdentityRemoved(netuid));
        }
        true
    }

    /// Clear [`IsNetworkMember`] rows for hotkeys that belonged to `netuid`.
    pub fn remove_network_is_network_member(
        netuid: NetUid,
        weight_meter: &mut WeightMeter,
        last_key: Option<Vec<u8>>,
    ) -> (bool, Option<Vec<u8>>) {
        let iter = match last_key {
            Some(raw_key) => Keys::<T>::iter_from(raw_key),
            None => Keys::<T>::iter(),
        };

        let (read_all, last_item) = Self::remove_storage_entries_for_netuid(
            weight_meter,
            iter,
            |(nu, _, _)| *nu == netuid,
            |(_, _, hotkey)| hotkey,
            |hotkey| IsNetworkMember::<T>::remove(hotkey, netuid),
            1,
        );

        (
            read_all,
            last_item.map(|(nu, uid, _)| Keys::<T>::hashed_key_for(nu, uid)),
        )
    }

    /// Zero root-network weight entries that pointed at the dissolved `netuid`.
    pub fn remove_network_update_weights_on_root(
        netuid: NetUid,
        weight_meter: &mut WeightMeter,
        last_key: Option<Vec<u8>>,
    ) -> (bool, Option<Vec<u8>>) {
        let netuid_u16 = u16::from(netuid);

        let root = NetUidStorageIndex::ROOT;
        let iter = match last_key {
            Some(raw_key) => Weights::<T>::iter_prefix_from(root, raw_key),
            None => Weights::<T>::iter_prefix(root),
        };

        /// Zero weight entries targeting `netuid_u16`; returns whether any entry changed.
        fn filter_root_weights_excluding_netuid(
            netuid_u16: u16,
            weights: &[(u16, u16)],
        ) -> (bool, Vec<(u16, u16)>) {
            let mut need_update = false;
            let mut filtered_weights = weights.to_vec();
            for (subnet_id, weight) in filtered_weights.iter_mut() {
                if *subnet_id == netuid_u16 && *weight != 0 {
                    need_update = true;
                    *weight = 0;
                }
            }
            (need_update, filtered_weights)
        }

        let (read_all, last_item) = Self::remove_storage_entries_for_netuid(
            weight_meter,
            iter,
            |_| true,
            |(uid, weights)| (uid, weights),
            |(uid, weights)| {
                let (update, filtered_weights) =
                    filter_root_weights_excluding_netuid(netuid_u16, weights);
                if update {
                    Weights::<T>::insert(root, *uid, filtered_weights);
                }
            },
            1,
        );

        (
            read_all,
            last_item.map(|key| Weights::<T>::hashed_key_for(root, key.0)),
        )
    }

    /// Clear [`ChildkeyTake`] entries for `netuid`.
    pub fn remove_network_childkey_take(
        netuid: NetUid,
        weight_meter: &mut WeightMeter,
        last_key: Option<Vec<u8>>,
    ) -> (bool, Option<Vec<u8>>) {
        let iter = match last_key {
            Some(raw_key) => ChildkeyTake::<T>::iter_from(raw_key),
            None => ChildkeyTake::<T>::iter(),
        };

        let (read_all, last_item) = Self::remove_storage_entries_for_netuid(
            weight_meter,
            iter,
            |(_, nu, _)| *nu == netuid,
            |(hot, _, _)| hot,
            |hot| ChildkeyTake::<T>::remove(hot, netuid),
            1,
        );

        (
            read_all,
            last_item.map(|(hot, nu, _)| ChildkeyTake::<T>::hashed_key_for(&hot, nu)),
        )
    }

    /// Clear child-key bindings for `netuid`.
    pub fn remove_network_childkeys(
        netuid: NetUid,
        weight_meter: &mut WeightMeter,
        last_key: Option<Vec<u8>>,
    ) -> (bool, Option<Vec<u8>>) {
        let iter = match last_key {
            Some(raw_key) => ChildKeys::<T>::iter_from(raw_key),
            None => ChildKeys::<T>::iter(),
        };

        let (read_all, last_item) = Self::remove_storage_entries_for_netuid(
            weight_meter,
            iter,
            |(_, nu, _)| *nu == netuid,
            |(hot, _, _)| hot,
            |hot| ChildKeys::<T>::remove(hot, netuid),
            1,
        );

        (
            read_all,
            last_item.map(|key| ChildKeys::<T>::hashed_key_for(&key.0, key.1)),
        )
    }

    /// Clear parent-key bindings for `netuid`.
    pub fn remove_network_parentkeys(
        netuid: NetUid,
        weight_meter: &mut WeightMeter,
        last_key: Option<Vec<u8>>,
    ) -> (bool, Option<Vec<u8>>) {
        let iter = match last_key {
            Some(raw_key) => ParentKeys::<T>::iter_from(raw_key),
            None => ParentKeys::<T>::iter(),
        };

        let (read_all, last_item) = Self::remove_storage_entries_for_netuid(
            weight_meter,
            iter,
            |(_, nu, _)| *nu == netuid,
            |(hot, _, _)| hot,
            |hot| ParentKeys::<T>::remove(hot, netuid),
            1,
        );

        (
            read_all,
            last_item.map(|key| ParentKeys::<T>::hashed_key_for(&key.0, key.1)),
        )
    }

    /// Clear last hotkey emission records for `netuid`.
    pub fn remove_network_last_hotkey_emission_on_netuid(
        netuid: NetUid,
        weight_meter: &mut WeightMeter,
        last_key: Option<Vec<u8>>,
    ) -> (bool, Option<Vec<u8>>) {
        let iter = match last_key {
            Some(raw_key) => LastHotkeyEmissionOnNetuid::<T>::iter_from(raw_key),
            None => LastHotkeyEmissionOnNetuid::<T>::iter(),
        };

        let (read_all, last_item) = Self::remove_storage_entries_for_netuid(
            weight_meter,
            iter,
            |(_, nu, _)| *nu == netuid,
            |(hot, _, _)| hot,
            |hot| LastHotkeyEmissionOnNetuid::<T>::remove(hot, netuid),
            1,
        );

        (
            read_all,
            last_item.map(|key| LastHotkeyEmissionOnNetuid::<T>::hashed_key_for(&key.0, key.1)),
        )
    }

    /// Clear last-epoch total hotkey alpha for `netuid`.
    pub fn remove_network_total_hotkey_alpha_last_epoch(
        netuid: NetUid,
        weight_meter: &mut WeightMeter,
        last_key: Option<Vec<u8>>,
    ) -> (bool, Option<Vec<u8>>) {
        let iter = match last_key {
            Some(raw_key) => TotalHotkeyAlphaLastEpoch::<T>::iter_from(raw_key),
            None => TotalHotkeyAlphaLastEpoch::<T>::iter(),
        };

        let (read_all, last_item) = Self::remove_storage_entries_for_netuid(
            weight_meter,
            iter,
            |(_, nu, _)| *nu == netuid,
            |(hot, _, _)| hot,
            |hot| TotalHotkeyAlphaLastEpoch::<T>::remove(hot, netuid),
            1,
        );

        (
            read_all,
            last_item.map(|(hot, nu, _)| TotalHotkeyAlphaLastEpoch::<T>::hashed_key_for(&hot, nu)),
        )
    }

    /// Clear per-hotkey transaction rate-limit stamps for `netuid`.
    pub fn remove_network_transaction_key_last_block(
        netuid: NetUid,
        weight_meter: &mut WeightMeter,
        last_key: Option<Vec<u8>>,
    ) -> (bool, Option<Vec<u8>>) {
        let iter = match last_key {
            Some(raw_key) => TransactionKeyLastBlock::<T>::iter_from(raw_key),
            None => TransactionKeyLastBlock::<T>::iter(),
        };

        let (read_all, last_item) = Self::remove_storage_entries_for_netuid(
            weight_meter,
            iter,
            |((_, nu, _), _)| *nu == netuid,
            |((hot, _, name), _)| (hot, name),
            |(hot, name)| TransactionKeyLastBlock::<T>::remove((hot.clone(), netuid, *name)),
            1,
        );

        (
            read_all,
            last_item.map(|((hot, _, name), _)| {
                TransactionKeyLastBlock::<T>::hashed_key_for((&hot, netuid, name))
            }),
        )
    }
}
