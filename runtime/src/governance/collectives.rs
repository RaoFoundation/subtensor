use alloc::vec::Vec;

use frame_support::pallet_prelude::*;
use pallet_multi_collective::{
    Collective, CollectiveInfo, CollectiveInspect, CollectivesInfo,
    weights::WeightInfo as MultiCollectiveWeightInfo,
};
use pallet_subtensor::root_registered::{OnRootRegistrationChange, RootRegisteredInspector};
use runtime_common::prod_or_fast;
use subtensor_runtime_common::{pad_name, time::DAYS};

use crate::{AccountId, BlockNumber, Runtime};

/// Keeps fresh subnet launches out of the Building rotation.
pub const MIN_SUBNET_AGE: BlockNumber = prod_or_fast!(180 * DAYS, 100);

/// Voting seats rotated into the Economic collective.
pub const ECONOMIC_SIZE: u32 = 16;

/// Voting seats rotated into the Building collective.
pub const BUILDING_SIZE: u32 = 16;

/// Cap on the EconomicEligible collective. Equal to the root subnet's
/// maximum UID count: membership mirrors the set of coldkeys with at
/// least one root-registered hotkey, so the worst case is one distinct
/// coldkey per root UID.
pub const ECONOMIC_ELIGIBLE_SIZE: u32 = 64;

/// Rotation cadence for ranked collectives.
const TERM_DURATION: BlockNumber = prod_or_fast!(60 * DAYS, 100);

/// Stable collective ids. Codec indices are consensus-facing.
#[derive(
    Copy,
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Debug,
    Encode,
    Decode,
    DecodeWithMemTracking,
    MaxEncodedLen,
    TypeInfo,
)]
pub enum CollectiveId {
    /// Accounts authorized to submit proposals on the triumvirate track.
    #[codec(index = 0)]
    Proposers,
    /// Three-member approval body for track 0.
    #[codec(index = 1)]
    Triumvirate,
    /// Top validators: one half of the collective oversight voter set.
    #[codec(index = 2)]
    Economic,
    /// Top subnet owners: one half of the collective oversight voter set.
    #[codec(index = 3)]
    Building,
    /// Staging set for the Economic collective. Membership is driven by
    /// `do_root_register` in `pallet-subtensor`; each rotation projects
    /// the top-`ECONOMIC_SIZE` from here into `Economic`.
    #[codec(index = 4)]
    EconomicEligible,
}

pub struct Collectives;
impl CollectivesInfo<BlockNumber, [u8; 32]> for Collectives {
    type Id = CollectiveId;

    fn collectives() -> impl Iterator<Item = Collective<Self::Id, BlockNumber, [u8; 32]>> {
        [
            Collective {
                id: CollectiveId::Proposers,
                info: CollectiveInfo {
                    name: pad_name(b"proposers"),
                    min_members: 1,
                    max_members: Some(20),
                    term_duration: None,
                },
            },
            Collective {
                id: CollectiveId::Triumvirate,
                info: CollectiveInfo {
                    name: pad_name(b"triumvirate"),
                    min_members: 3,
                    max_members: Some(3),
                    term_duration: None,
                },
            },
            Collective {
                id: CollectiveId::Economic,
                info: CollectiveInfo {
                    name: pad_name(b"economic"),
                    min_members: ECONOMIC_SIZE,
                    max_members: Some(ECONOMIC_SIZE),
                    term_duration: Some(TERM_DURATION),
                },
            },
            Collective {
                id: CollectiveId::Building,
                info: CollectiveInfo {
                    name: pad_name(b"building"),
                    min_members: BUILDING_SIZE,
                    max_members: Some(BUILDING_SIZE),
                    term_duration: Some(TERM_DURATION),
                },
            },
            Collective {
                id: CollectiveId::EconomicEligible,
                info: CollectiveInfo {
                    name: pad_name(b"economic_eligible"),
                    min_members: 0,
                    max_members: Some(ECONOMIC_ELIGIBLE_SIZE),
                    term_duration: None,
                },
            },
        ]
        .into_iter()
    }
}

/// Keeps the Economic eligibility pool aligned with root registration.
///
/// Failures are logged instead of blocking root-register or hotkey-swap
/// calls; `try_state` checks the invariant afterwards.
pub struct EconomicEligibleSync;

impl OnRootRegistrationChange<AccountId> for EconomicEligibleSync {
    fn on_added(coldkey: &AccountId) {
        if let Err(err) = pallet_multi_collective::Pallet::<Runtime>::do_add_member(
            CollectiveId::EconomicEligible,
            coldkey.clone(),
        ) {
            log::error!(
                target: "runtime::economic-eligible-sync",
                "do_add_member failed for {:?}: {:?}",
                coldkey,
                err,
            );
        }
    }

    fn on_removed(coldkey: &AccountId) {
        if let Err(err) = pallet_multi_collective::Pallet::<Runtime>::do_remove_member(
            CollectiveId::EconomicEligible,
            coldkey.clone(),
        ) {
            log::error!(
                target: "runtime::economic-eligible-sync",
                "do_remove_member failed for {:?}: {:?}",
                coldkey,
                err,
            );
        }
    }

    fn on_added_weight() -> Weight {
        <Runtime as pallet_multi_collective::Config>::WeightInfo::do_add_member()
    }

    fn on_removed_weight() -> Weight {
        <Runtime as pallet_multi_collective::Config>::WeightInfo::do_remove_member()
    }
}

/// Lets `pallet-subtensor` verify its root-registration invariant.
pub struct EconomicEligibleInspector;

impl RootRegisteredInspector<AccountId> for EconomicEligibleInspector {
    fn members() -> Option<Vec<AccountId>> {
        Some(
            <pallet_multi_collective::Pallet<Runtime> as CollectiveInspect<
                AccountId,
                CollectiveId,
            >>::members_of(CollectiveId::EconomicEligible),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codec::Encode;

    #[test]
    fn collective_id_codec_indices_are_pinned() {
        assert_eq!(CollectiveId::Proposers.encode(), vec![0]);
        assert_eq!(CollectiveId::Triumvirate.encode(), vec![1]);
        assert_eq!(CollectiveId::Economic.encode(), vec![2]);
        assert_eq!(CollectiveId::Building.encode(), vec![3]);
        assert_eq!(CollectiveId::EconomicEligible.encode(), vec![4]);
    }
}
