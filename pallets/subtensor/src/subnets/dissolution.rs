use super::*;
use frame_support::weights::WeightMeter;
use subtensor_runtime_common::{NetUid, NetUidStorageIndex, clear_prefix_with_meter};
use subtensor_swap_interface::SwapHandler;
/// Enum for the dissolve cleanup phase.
#[derive(Encode, Decode, TypeInfo, Clone, PartialEq, Eq, Debug, DecodeWithMemTracking)]
pub enum DissolveCleanupPhase {
    /// Phase 1.1: Remove root dividend claimable entries for the subnet.
    SubnetRootDividendsRootClaimable,
    /// Phase 1.2: Remove root dividend claimed entries for the subnet.
    SubnetRootDividendsRootClaimed,
    /// Phase 2.1: Get the total alpha value for the subnet.
    AlphaInOutStakesGetTotalAlphaValue,
    /// Phase 2.2: Destroy alpha in and out stakes for the subnet.
    AlphaInOutStakesSettleStakes,
    /// Phase 2.3: Clean alpha entries for the subnet.
    AlphaInOutStakesAlpha,
    /// Phase 2.4: Clear hotkey totals for the subnet.
    AlphaInOutStakesHotkeyTotals,
    /// Phase 2.5: Clear locks for the subnet.
    AlphaInOutStakesLocks,
    /// Phase 2.6: Clear locks for the subnet.
    AlphaInOutStakesDecayingLocks,
    /// Phase 2.7: Destroy alpha in and out stakes for the subnet.
    AlphaInOutStakes,
    /// Phase 3: Clear protocol liquidity for the subnet on the swap layer.
    ProtocolLiquidity,
    /// Phase 4: Remove scalar `Network*` parameters, then continue with map and index cleanup phases.
    PurgeNetuid,
    /// Phase 5.1: Remove is network member entries for the subnet.
    NetworkIsNetworkMember,
    /// Phase 5.2: Recovery / legacy: scalar `Network*` removal; the hook advances to map cleanup like `PurgeNetuid` after `remove_network_parameters` completes.
    NetworkParameters,
    /// Phase 5.3: Remove map-backed subnet storage (keys, axons, per-mechanism weights, etc.).
    NetworkMapParameters,
    /// Phase 5.4: Clear root-network weight entries referencing this netuid.
    NetworkUpdateWeightsOnRoot,
    /// Phase 5.5: Remove childkey take entries for this netuid.
    NetworkChildkeyTake,
    /// Phase 5.6: Remove child key bindings for this netuid.
    NetworkChildkeys,
    /// Phase 5.7: Remove parent key bindings for this netuid.
    NetworkParentkeys,
    /// Phase 5.8: Remove last hotkey emission records for this netuid.
    NetworkLastHotkeyEmissionOnNetuid,
    /// Phase 5.9: Remove total hotkey alpha last epoch entries for this netuid.
    NetworkTotalHotkeyAlphaLastEpoch,
    /// Phase 5.10: Remove transaction key last-block rate limit entries for this netuid.
    NetworkTransactionKeyLastBlock,
    /// Phase 5.11: Remove lock entries for this netuid.
    NetworkLock,
    /// Phase 5.12: Remove decaying lock entries for this netuid.
    NetworkDecayingLock,
    /// Phase 0: Convert each validator's beta-basket holding on this subnet into the fund's
    /// root (TAO) slot. Must finish before stake / protocol-liquidity teardown so the AMM and
    /// escrow alpha still exist. Appended (not inserted) so in-flight cleanup discriminants stay
    /// stable; new dissolves start here via [`Default`].
    SubnetBasketHoldingsToRoot,
    /// Phase 5.13: Recycle and drop queued [`PendingBasketDeposits`] credits originating
    /// from this netuid. Without this, a stale credit (dust can sit queued indefinitely)
    /// would survive dissolution and — once a new subnet reuses the netuid, making
    /// `if_subnet_exist` true again — deposit the old subnet's alpha against the new
    /// subnet's pool. Credits are recycled (issuance conservation) rather than silently
    /// deleted. Must complete before cleanup finishes because netuid reuse is only
    /// possible after that. Appended (not inserted) so in-flight cleanup discriminants
    /// stay stable.
    NetworkPendingBasketDeposits,
}

impl Default for DissolveCleanupPhase {
    fn default() -> Self {
        Self::SubnetBasketHoldingsToRoot
    }
}

#[crate::freeze_struct("c524ea54893ae91a")]
#[derive(Encode, Decode, TypeInfo, Clone, PartialEq, Eq, Debug, DecodeWithMemTracking)]
pub struct DissolveCleanupStatus {
    pub netuid: NetUid,
    pub phase: DissolveCleanupPhase,
    pub last_key: Option<Vec<u8>>,
    pub subnet_total_alpha_value: Option<u128>,
    pub subnet_distributed_tao: Option<u128>,
}

impl DissolveCleanupStatus {
    pub fn new(netuid: NetUid) -> Self {
        Self {
            netuid,
            phase: DissolveCleanupPhase::default(),
            last_key: None,
            subnet_total_alpha_value: None,
            subnet_distributed_tao: None,
        }
    }

    pub fn set_phase(&mut self, phase: DissolveCleanupPhase) {
        self.phase = phase;
    }
}

impl<T: Config> Pallet<T> {
    /// Facilitates the removal of a user's subnetwork.
    ///
    /// # Arguments
    /// * `origin`: ('T::RuntimeOrigin'): The calling origin. Must be signed.
    /// * `netuid`: ('u16'): The unique identifier of the network to be removed.
    ///
    /// # Events
    /// * `NetworkRemoved`: Emitted when a network is successfully removed.
    ///
    /// # Errors
    /// * `MechanismDoesNotExist`: If the specified network does not exist.
    /// * `NotSubnetOwner`: If the caller does not own the specified subnet.
    ///
    pub fn do_dissolve_network(netuid: NetUid) -> dispatch::DispatchResult {
        // --- The network exists?
        ensure!(
            Self::if_subnet_exist(netuid) && netuid != NetUid::ROOT,
            Error::<T>::SubnetNotExists
        );

        // Since TotalStake is updated on this level, purge reservoirs here into reserves and TotalStake
        let reservoir_tao = T::SwapInterface::protocol_tao_reservoir(netuid);
        let reservoir_alpha = T::SwapInterface::protocol_alpha_reservoir(netuid);
        T::SwapInterface::clear_protocol_liquidity_reservoirs(netuid);
        Self::increase_provided_tao_reserve(netuid, reservoir_tao);
        Self::increase_provided_alpha_reserve(netuid, reservoir_alpha);
        if !reservoir_tao.is_zero() {
            TotalStake::<T>::mutate(|total| {
                *total = total.saturating_add(reservoir_tao);
            });
        }

        let mut dissolved_networks = DissolveCleanupQueue::<T>::get();
        ensure!(
            !dissolved_networks.contains(&netuid),
            Error::<T>::NetworkDissolveAlreadyQueued
        );

        // Basket holdings are converted to each fund's root slot in the metered
        // `SubnetBasketHoldingsToRoot` cleanup phase (before stake / AMM teardown).

        // Just remove the network from the added networks, it is used to check if the network is existed.
        NetworksAdded::<T>::remove(netuid);
        // Reduce the total networks count.
        TotalNetworks::<T>::mutate(|n: &mut u16| *n = n.saturating_sub(1));
        TotalStake::<T>::mutate(|total| *total = total.saturating_sub(SubnetTAO::<T>::get(netuid)));

        dissolved_networks.push(netuid);
        DissolveCleanupQueue::<T>::set(dissolved_networks);

        log::debug!("NetworkRemoved( netuid:{netuid:?} )");

        // --- Emit the NetworkRemoved event
        Self::deposit_event(Event::NetworkRemoved(netuid));

        Ok(())
    }

    pub fn remove_network_map_parameters(netuid: NetUid, weight_meter: &mut WeightMeter) -> bool {
        let write_weight = T::DbWeight::get().writes(1);

        let result = clear_prefix_with_meter(weight_meter, write_weight, |limit| {
            Keys::<T>::clear_prefix(netuid, limit, None)
        }) && clear_prefix_with_meter(weight_meter, write_weight, |limit| {
            Uids::<T>::clear_prefix(netuid, limit, None)
        }) && clear_prefix_with_meter(weight_meter, write_weight, |limit| {
            VotingPower::<T>::clear_prefix(netuid, limit, None)
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
            let mechanism_weight = T::DbWeight::get().reads_writes(1, 3);
            if !weight_meter.can_consume(mechanism_weight) {
                return false;
            }
            weight_meter.consume(mechanism_weight);
            let netuid_index = Self::get_mechanism_storage_index(netuid, subid.into());

            LastUpdate::<T>::remove(netuid_index);
            Incentive::<T>::remove(netuid_index);
            ConsensusByMechanism::<T>::remove(netuid_index);

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

    pub fn remove_network_parameters(netuid: NetUid, weight_meter: &mut WeightMeter) -> bool {
        // Flat write charge for the `::remove(netuid)` list below. Bump this when
        // adding or removing entries from that list so the weight stays in step.
        let removal_weight = T::DbWeight::get().writes(86);
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
        LiquidAlphaConsensusMode::<T>::remove(netuid);
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
        TotalVotingPower::<T>::remove(netuid);
        VotingPowerTrackingEnabled::<T>::remove(netuid);
        VotingPowerDisableAtBlock::<T>::remove(netuid);
        VotingPowerEmaAlpha::<T>::remove(netuid);

        if SubnetIdentitiesV3::<T>::contains_key(netuid) {
            SubnetIdentitiesV3::<T>::remove(netuid);
            Self::deposit_event(Event::SubnetIdentityRemoved(netuid));
        }
        true
    }

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

        fn filter_weights(netuid_u16: u16, weights: &[(u16, u16)]) -> (bool, Vec<(u16, u16)>) {
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
                let (update, filtered_weights) = filter_weights(netuid_u16, weights);
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

    pub fn remove_network_pending_basket_deposits(
        netuid: NetUid,
        weight_meter: &mut WeightMeter,
        last_key: Option<Vec<u8>>,
    ) -> (bool, Option<Vec<u8>>) {
        let iter = match last_key {
            Some(raw_key) => PendingBasketDeposits::<T>::iter_from(raw_key),
            None => PendingBasketDeposits::<T>::iter(),
        };

        // Recycle + remove: three writes typical (PendingBasketDeposits remove, AlphaAssets
        // recycle, and SubnetAlphaOut when it still exists). Charge the upper bound.
        let (read_all, last_item) = Self::remove_storage_entries_for_netuid(
            weight_meter,
            iter,
            |(_, nu, _)| *nu == netuid,
            |(hot, _, alpha)| (hot, alpha),
            |(hot, alpha)| Self::drop_pending_basket_deposit(hot, netuid, *alpha),
            3,
        );

        (
            read_all,
            last_item.map(|(hot, nu, _)| PendingBasketDeposits::<T>::hashed_key_for(&hot, nu)),
        )
    }

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

    pub fn remove_data_for_dissolved_networks(remaining_weight: Weight) -> Weight {
        let w = T::DbWeight::get().writes(1);
        let r = T::DbWeight::get().reads(1);
        let mut weight_meter = frame_support::weights::WeightMeter::with_limit(remaining_weight);

        // complete unfinished network cleanup at first if any
        if let Some(mut status) = CurrentDissolveCleanupStatus::<T>::get() {
            let (cleanup_completed, weight) =
                Self::clean_up_data_for_one_dissolved_network(&mut weight_meter, &mut status);
            if cleanup_completed {
                DissolveCleanupQueue::<T>::mutate(|queue| {
                    queue.retain(|queued_netuid| *queued_netuid != status.netuid);
                });
                CurrentDissolveCleanupStatus::<T>::kill();
                return weight.saturating_add(T::DbWeight::get().writes(2));
            }
            return weight;
        }

        if !weight_meter.can_consume(r) {
            return weight_meter.consumed();
        }
        weight_meter.consume(r);

        let dissolved_networks = DissolveCleanupQueue::<T>::get();
        if let Some(netuid) = dissolved_networks.first() {
            if !weight_meter.can_consume(w) {
                return weight_meter.consumed();
            }
            weight_meter.consume(w);

            let mut status = DissolveCleanupStatus::new(*netuid);
            CurrentDissolveCleanupStatus::<T>::set(Some(status.clone()));

            let (cleanup_completed, _weight) =
                Self::clean_up_data_for_one_dissolved_network(&mut weight_meter, &mut status);

            if cleanup_completed {
                DissolveCleanupQueue::<T>::mutate(|queue| {
                    queue.retain(|queued_netuid| *queued_netuid != status.netuid);
                });
                CurrentDissolveCleanupStatus::<T>::kill();
                weight_meter.consume(T::DbWeight::get().writes(2));
            }
        }

        weight_meter.consumed()
    }

    // try use all weight available to clean up data for one dissolved network based on the status
    pub fn clean_up_data_for_one_dissolved_network(
        weight_meter: &mut WeightMeter,
        status: &mut DissolveCleanupStatus,
    ) -> (bool, Weight) {
        let r = T::DbWeight::get().reads(1);

        let netuid = status.netuid;

        if !weight_meter.can_consume(r) {
            return (false, weight_meter.consumed());
        }

        // if one phase is done or exit because of weight limit
        let mut phase_done = true;
        let mut cleanup_completed = false;
        // only reason for phase_done to be false is if the weight limit is reached
        while phase_done {
            // let phase = status.phase.clone();
            log::debug!(
                "dissolved_networks phase: {:?} for netuid: {:?}",
                &status.phase,
                netuid
            );

            let done = match &status.phase {
                DissolveCleanupPhase::SubnetBasketHoldingsToRoot => {
                    let (done, new_key) = Self::convert_subnet_basket_holdings_to_root(
                        netuid,
                        weight_meter,
                        status.last_key.clone(),
                    );

                    if done {
                        status.set_phase(DissolveCleanupPhase::SubnetRootDividendsRootClaimable);
                        status.last_key = None;
                    } else {
                        status.last_key = new_key;
                    }
                    done
                }

                DissolveCleanupPhase::SubnetRootDividendsRootClaimable => {
                    let (done, new_key) = Self::clean_up_root_claimable_for_subnet(
                        netuid,
                        weight_meter,
                        status.last_key.clone(),
                    );

                    if done {
                        status.set_phase(DissolveCleanupPhase::SubnetRootDividendsRootClaimed);
                        status.last_key = None;
                    } else {
                        status.last_key = new_key;
                    }
                    done
                }

                DissolveCleanupPhase::SubnetRootDividendsRootClaimed => {
                    let done = Self::clean_up_root_claimed_for_subnet(netuid, weight_meter);

                    if done {
                        status.set_phase(DissolveCleanupPhase::AlphaInOutStakesGetTotalAlphaValue);
                        status.last_key = None;
                    }
                    done
                }

                DissolveCleanupPhase::AlphaInOutStakesGetTotalAlphaValue => {
                    let (done, new_key) = Self::destroy_alpha_in_out_stakes_get_total_alpha_value(
                        netuid,
                        weight_meter,
                        status.last_key.clone(),
                        status,
                    );
                    if done {
                        status.subnet_distributed_tao = Some(0);
                        status.set_phase(DissolveCleanupPhase::AlphaInOutStakesSettleStakes);
                        status.last_key = None;
                        weight_meter.consume(T::DbWeight::get().writes(2));
                    } else {
                        status.last_key = new_key;
                    }
                    done
                }

                DissolveCleanupPhase::AlphaInOutStakesSettleStakes => {
                    let (done, new_key) = Self::destroy_alpha_in_out_stakes_settle_stakes(
                        netuid,
                        weight_meter,
                        status.last_key.clone(),
                        status,
                    );
                    if done {
                        status.set_phase(DissolveCleanupPhase::AlphaInOutStakesAlpha);
                        status.last_key = None;
                    } else {
                        status.last_key = new_key;
                    }
                    done
                }

                DissolveCleanupPhase::AlphaInOutStakesAlpha => {
                    let (done, new_key) = Self::destroy_alpha_in_out_stakes_clean_alpha(
                        netuid,
                        weight_meter,
                        status.last_key.clone(),
                    );
                    if done {
                        status.set_phase(DissolveCleanupPhase::AlphaInOutStakesHotkeyTotals);
                        status.last_key = None;
                    } else {
                        status.last_key = new_key;
                    }
                    done
                }

                DissolveCleanupPhase::AlphaInOutStakesHotkeyTotals => {
                    let (done, new_key) = Self::destroy_alpha_in_out_stakes_clear_hotkey_totals(
                        netuid,
                        weight_meter,
                        status.last_key.clone(),
                    );

                    if done {
                        status.set_phase(DissolveCleanupPhase::AlphaInOutStakesLocks);
                        status.last_key = None;
                    } else {
                        status.last_key = new_key;
                    }
                    done
                }

                DissolveCleanupPhase::AlphaInOutStakesLocks => {
                    let (done, new_key) = Self::destroy_alpha_in_out_stakes_clear_locks(
                        netuid,
                        weight_meter,
                        status.last_key.clone(),
                    );
                    if done {
                        status.set_phase(DissolveCleanupPhase::AlphaInOutStakesDecayingLocks);
                        status.last_key = None;
                    } else {
                        status.last_key = new_key;
                    }
                    done
                }
                DissolveCleanupPhase::AlphaInOutStakesDecayingLocks => {
                    let (done, new_key) = Self::destroy_alpha_in_out_stakes_clear_decaying_locks(
                        netuid,
                        weight_meter,
                        status.last_key.clone(),
                    );
                    if done {
                        status.set_phase(DissolveCleanupPhase::AlphaInOutStakes);
                        status.last_key = None;
                    } else {
                        status.last_key = new_key;
                    }
                    done
                }

                DissolveCleanupPhase::AlphaInOutStakes => {
                    let done = Self::destroy_alpha_in_out_stakes(netuid, weight_meter, status);
                    if done {
                        status.set_phase(DissolveCleanupPhase::ProtocolLiquidity);
                        status.last_key = None;
                    }
                    done
                }

                DissolveCleanupPhase::ProtocolLiquidity => {
                    let done = T::SwapInterface::clear_protocol_liquidity(netuid, weight_meter);

                    if done {
                        status.set_phase(DissolveCleanupPhase::PurgeNetuid);
                        status.last_key = None;
                    }
                    done
                }

                DissolveCleanupPhase::PurgeNetuid => {
                    let done = T::CommitmentsInterface::purge_netuid(netuid, weight_meter);

                    if done {
                        status.set_phase(DissolveCleanupPhase::NetworkIsNetworkMember);
                        status.last_key = None;
                    }
                    done
                }
                DissolveCleanupPhase::NetworkIsNetworkMember => {
                    let (done, new_key) = Self::remove_network_is_network_member(
                        netuid,
                        weight_meter,
                        status.last_key.clone(),
                    );

                    if done {
                        status.set_phase(DissolveCleanupPhase::NetworkParameters);
                        status.last_key = None;
                    } else {
                        status.last_key = new_key;
                    }
                    done
                }
                DissolveCleanupPhase::NetworkParameters => {
                    let done = Self::remove_network_parameters(netuid, weight_meter);

                    if done {
                        status.set_phase(DissolveCleanupPhase::NetworkMapParameters);
                        status.last_key = None;
                    }
                    done
                }
                DissolveCleanupPhase::NetworkMapParameters => {
                    let done = Self::remove_network_map_parameters(netuid, weight_meter);

                    if done {
                        status.set_phase(DissolveCleanupPhase::NetworkUpdateWeightsOnRoot);
                        status.last_key = None;
                    }
                    done
                }
                DissolveCleanupPhase::NetworkUpdateWeightsOnRoot => {
                    let (done, new_key) = Self::remove_network_update_weights_on_root(
                        netuid,
                        weight_meter,
                        status.last_key.clone(),
                    );

                    if done {
                        status.set_phase(DissolveCleanupPhase::NetworkChildkeyTake);
                        status.last_key = None;
                    } else {
                        status.last_key = new_key;
                    }
                    done
                }
                DissolveCleanupPhase::NetworkChildkeyTake => {
                    let (done, new_key) = Self::remove_network_childkey_take(
                        netuid,
                        weight_meter,
                        status.last_key.clone(),
                    );

                    if done {
                        status.set_phase(DissolveCleanupPhase::NetworkChildkeys);
                        status.last_key = None;
                    } else {
                        status.last_key = new_key;
                    }
                    done
                }
                DissolveCleanupPhase::NetworkChildkeys => {
                    let (done, new_key) = Self::remove_network_childkeys(
                        netuid,
                        weight_meter,
                        status.last_key.clone(),
                    );

                    if done {
                        status.set_phase(DissolveCleanupPhase::NetworkParentkeys);
                        status.last_key = None;
                    } else {
                        status.last_key = new_key;
                    }
                    done
                }
                DissolveCleanupPhase::NetworkParentkeys => {
                    let (done, new_key) = Self::remove_network_parentkeys(
                        netuid,
                        weight_meter,
                        status.last_key.clone(),
                    );

                    if done {
                        status.set_phase(DissolveCleanupPhase::NetworkLastHotkeyEmissionOnNetuid);
                        status.last_key = None;
                    } else {
                        status.last_key = new_key;
                    }
                    done
                }
                DissolveCleanupPhase::NetworkLastHotkeyEmissionOnNetuid => {
                    let (done, new_key) = Self::remove_network_last_hotkey_emission_on_netuid(
                        netuid,
                        weight_meter,
                        status.last_key.clone(),
                    );

                    if done {
                        status.set_phase(DissolveCleanupPhase::NetworkTotalHotkeyAlphaLastEpoch);
                        status.last_key = None;
                    } else {
                        status.last_key = new_key;
                    }
                    done
                }
                DissolveCleanupPhase::NetworkTotalHotkeyAlphaLastEpoch => {
                    let (done, new_key) = Self::remove_network_total_hotkey_alpha_last_epoch(
                        netuid,
                        weight_meter,
                        status.last_key.clone(),
                    );

                    if done {
                        status.set_phase(DissolveCleanupPhase::NetworkTransactionKeyLastBlock);
                        status.last_key = None;
                    } else {
                        status.last_key = new_key;
                    }
                    done
                }
                DissolveCleanupPhase::NetworkTransactionKeyLastBlock => {
                    let (done, new_key) = Self::remove_network_transaction_key_last_block(
                        netuid,
                        weight_meter,
                        status.last_key.clone(),
                    );
                    if done {
                        status.set_phase(DissolveCleanupPhase::NetworkLock);
                        status.last_key = None;
                    } else {
                        status.last_key = new_key;
                    }
                    done
                }
                DissolveCleanupPhase::NetworkLock => {
                    let (done, new_key) =
                        Self::remove_network_lock(netuid, weight_meter, status.last_key.clone());

                    if done {
                        status.set_phase(DissolveCleanupPhase::NetworkDecayingLock);
                        status.last_key = None;
                    } else {
                        status.last_key = new_key;
                    }
                    done
                }
                DissolveCleanupPhase::NetworkDecayingLock => {
                    let (done, new_key) = Self::remove_network_decaying_lock(
                        netuid,
                        weight_meter,
                        status.last_key.clone(),
                    );

                    if done {
                        status.set_phase(DissolveCleanupPhase::NetworkPendingBasketDeposits);
                        status.last_key = None;
                    } else {
                        status.last_key = new_key;
                    }
                    done
                }
                DissolveCleanupPhase::NetworkPendingBasketDeposits => {
                    let (done, new_key) = Self::remove_network_pending_basket_deposits(
                        netuid,
                        weight_meter,
                        status.last_key.clone(),
                    );

                    // if all phases are done, remove the network from the dissolved networks list and emit the event
                    if done {
                        cleanup_completed = true;
                    } else {
                        status.last_key = new_key;
                    }
                    done
                }
            };

            phase_done = done;

            if cleanup_completed {
                Self::deposit_event(Event::NetworkDissolveCleanupCompleted { netuid });
                break;
            }

            CurrentDissolveCleanupStatus::<T>::set(Some(status.clone()));
        }

        (cleanup_completed, weight_meter.consumed())
    }

    pub fn process_network_registration_queue() -> Weight {
        let db_weight = T::DbWeight::get();
        let queue = NetworkRegistrationQueue::<T>::get();
        let mut weight = db_weight.reads(1);

        for (index, info) in queue.iter().enumerate() {
            // just complete one registration at a time since on_idle just complete one network dissolve cleanup
            // if one registration fails, then try next one. it could be not align with the order of registration in the queue
            match Self::set_new_network_state(
                &info.coldkey,
                &info.hotkey,
                info.mechid,
                info.identity.clone(),
                info.lock_amount,
                info.median_subnet_alpha_price,
                Some(info.lock_id),
            ) {
                Ok(post_info) => {
                    NetworkRegistrationQueue::<T>::mutate(|queue| queue.remove(index));
                    weight.saturating_accrue(db_weight.reads_writes(1, 1));
                    weight.saturating_accrue(post_info.actual_weight.unwrap_or_else(Weight::zero));
                    return weight;
                }
                Err(_) => {
                    log::error!(
                        "Failed to set new network state for coldkey: {:?}, hotkey: {:?}",
                        info.coldkey,
                        info.hotkey
                    );
                    continue;
                }
            }
        }

        weight
    }
}
