//! Dissolve cleanup phase machine and persisted resume status.
//!
//! [`DissolveCleanupStatus`] is stored in [`CurrentDissolveCleanupStatus`] so
//! weight-metered dissolve cleanup can resume across blocks.

use super::*;
use subtensor_runtime_common::NetUid;

/// Ordered phases of multi-block dissolve cleanup for one `netuid`.
///
/// Variant order is part of the on-chain encoding of [`DissolveCleanupStatus`];
/// do not reorder.
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
}

impl Default for DissolveCleanupPhase {
    fn default() -> Self {
        Self::SubnetRootDividendsRootClaimable
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
    /// Start cleanup for `netuid` at the first phase with empty resume cursor.
    pub fn new(netuid: NetUid) -> Self {
        Self {
            netuid,
            phase: DissolveCleanupPhase::default(),
            last_key: None,
            subnet_total_alpha_value: None,
            subnet_distributed_tao: None,
        }
    }

    /// Advance to `phase` (caller clears `last_key` when entering a new map walk).
    pub fn set_phase(&mut self, phase: DissolveCleanupPhase) {
        self.phase = phase;
    }
}
