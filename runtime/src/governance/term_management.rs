use alloc::vec::Vec;

use frame_support::pallet_prelude::*;
use pallet_multi_collective::{
    CollectiveInspect, OnNewTerm, Pallet as MultiCollective,
    weights::WeightInfo as MultiCollectiveWeightInfo,
};
use pallet_subtensor::{Pallet as Subtensor, *};
use sp_runtime::traits::UniqueSaturatedInto;
use substrate_fixed::types::{I96F32, U64F64};

use crate::{AccountId, BlockNumber, Runtime};

use super::collectives::{BUILDING_SIZE, CollectiveId, ECONOMIC_SIZE, MIN_SUBNET_AGE};
use super::weights::{SubstrateWeight as GovernanceWeight, WeightInfo as GovernanceWeightInfo};

/// Minimum root-registered EMA samples before Economic eligibility.
/// With the current sampler cadence, 210 is roughly 30 days.
pub const ECONOMIC_ELIGIBILITY_THRESHOLD: u32 = 210;

/// Runtime rotation policy for rotating collectives.
pub struct TermManagement;

impl OnNewTerm<CollectiveId> for TermManagement {
    fn weight() -> Weight {
        [
            GovernanceWeight::<Runtime>::rotate_economic(),
            GovernanceWeight::<Runtime>::rotate_building(),
        ]
        .into_iter()
        .max_by_key(Weight::ref_time)
        .unwrap_or_default()
    }

    fn on_new_term(collective_id: CollectiveId) -> Weight {
        // Curated collectives are managed outside this rotation policy.
        match collective_id {
            CollectiveId::Economic => Self::rotate_economic(),
            CollectiveId::Building => Self::rotate_building(),
            _ => Weight::zero(),
        }
    }
}

impl TermManagement {
    pub(crate) fn rotate_economic() -> Weight {
        let (members, query_weight) = Self::top_validators(ECONOMIC_SIZE);
        Self::apply_rotation(CollectiveId::Economic, members, query_weight)
    }

    pub(crate) fn rotate_building() -> Weight {
        let (members, query_weight) = Self::top_subnet_owners(BUILDING_SIZE, MIN_SUBNET_AGE);
        Self::apply_rotation(CollectiveId::Building, members, query_weight)
    }

    /// Top validator coldkeys by smoothed root-registered value.
    pub fn top_validators(n: u32) -> (Vec<AccountId>, Weight) {
        let db = <Runtime as frame_system::Config>::DbWeight::get();
        let eligible =
            <MultiCollective<Runtime> as CollectiveInspect<AccountId, CollectiveId>>::members_of(
                CollectiveId::EconomicEligible,
            );
        let mut weight = db.reads(1);

        let entries: Vec<(AccountId, U64F64)> = eligible
            .into_iter()
            .filter_map(|coldkey| {
                weight.saturating_accrue(db.reads(1));
                let state = RootRegisteredEma::<Runtime>::get(&coldkey);
                (state.samples >= ECONOMIC_ELIGIBILITY_THRESHOLD).then_some((coldkey, state.ema))
            })
            .collect();

        (rank_top_n(entries, n), weight)
    }

    /// Top subnet-owner coldkeys by their best mature subnet price.
    pub fn top_subnet_owners(n: u32, min_age: BlockNumber) -> (Vec<AccountId>, Weight) {
        let mut weight = Weight::zero();
        let now: u64 = <frame_system::Pallet<Runtime>>::block_number().into();
        let min_age_u64: u64 = min_age.into();

        let mut entries: Vec<(AccountId, I96F32)> = Vec::new();
        for netuid in Subtensor::<Runtime>::get_all_subnet_netuids() {
            weight.saturating_accrue(<Runtime as frame_system::Config>::DbWeight::get().reads(3));
            let registered_at: u64 = NetworkRegisteredAt::<Runtime>::get(netuid);
            if now.saturating_sub(registered_at) < min_age_u64 {
                continue;
            }
            let price = SubnetMovingPrice::<Runtime>::get(netuid);
            let owner = SubnetOwner::<Runtime>::get(netuid);
            merge_owner_by_highest_price(&mut entries, owner, price);
        }

        (rank_top_n(entries, n), weight)
    }

    /// Apply a rotated membership through the collective pallet.
    fn apply_rotation(
        collective_id: CollectiveId,
        members: Vec<AccountId>,
        query_weight: Weight,
    ) -> Weight {
        let result = MultiCollective::<Runtime>::do_set_members(collective_id, members);

        if let Err(err) = result {
            log::error!(
                target: "runtime::collective-management",
                "rotation failed for {:?}: {:?}",
                collective_id,
                err,
            );
        }

        query_weight
            .saturating_add(<Runtime as pallet_multi_collective::Config>::WeightInfo::set_members())
    }
}

/// Sort by descending score and return the first `n` keys.
fn rank_top_n<K, S: Ord>(mut entries: Vec<(K, S)>, n: u32) -> Vec<K> {
    entries.sort_by(|a, b| b.1.cmp(&a.1));
    entries.truncate(n.unique_saturated_into());
    entries.into_iter().map(|(k, _)| k).collect()
}

/// Keep only an owner's highest observed subnet price.
fn merge_owner_by_highest_price<A: PartialEq>(
    entries: &mut Vec<(A, I96F32)>,
    owner: A,
    price: I96F32,
) {
    if let Some(existing) = entries.iter_mut().find(|(o, _)| *o == owner) {
        if price > existing.1 {
            existing.1 = price;
        }
    } else {
        entries.push((owner, price));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use pallet_subtensor::root_registered::EmaState;
    use sp_runtime::BuildStorage;
    use subtensor_runtime_common::NetUid;

    fn new_test_ext() -> sp_io::TestExternalities {
        let storage = match (crate::RuntimeGenesisConfig {
            sudo: pallet_sudo::GenesisConfig { key: None },
            ..Default::default()
        })
        .build_storage()
        {
            Ok(storage) => storage,
            Err(err) => panic!("failed to build test storage: {err:?}"),
        };
        let mut ext: sp_io::TestExternalities = storage.into();
        ext.execute_with(|| crate::System::set_block_number(1));
        ext
    }

    fn account(seed: u8) -> AccountId {
        AccountId::from([seed; 32])
    }

    fn accounts(start: u8, count: u32) -> Vec<AccountId> {
        (0..count)
            .map(|offset| account(start + offset as u8))
            .collect()
    }

    fn rank_entry(key: u32, score: u64) -> (u32, U64F64) {
        (key, U64F64::from_num(score))
    }

    fn price(value: i64) -> I96F32 {
        I96F32::from_num(value)
    }

    fn set_members(collective_id: CollectiveId, members: Vec<AccountId>) {
        assert!(
            MultiCollective::<Runtime>::set_members(
                frame_system::RawOrigin::Root.into(),
                collective_id,
                members,
            )
            .is_ok()
        );
    }

    fn members_of(collective_id: CollectiveId) -> Vec<AccountId> {
        <MultiCollective<Runtime> as CollectiveInspect<AccountId, CollectiveId>>::members_of(
            collective_id,
        )
    }

    fn set_ema(coldkey: &AccountId, ema: u64, samples: u32) {
        RootRegisteredEma::<Runtime>::insert(
            coldkey,
            EmaState {
                ema: U64F64::from_num(ema),
                samples,
            },
        );
    }

    fn seed_subnet(netuid: NetUid, owner: AccountId, price: i64, registered_at: u64) {
        Subtensor::<Runtime>::init_new_network(netuid, 1);
        NetworkRegisteredAt::<Runtime>::insert(netuid, registered_at);
        SubnetMovingPrice::<Runtime>::insert(netuid, I96F32::from_num(price));
        SubnetOwner::<Runtime>::insert(netuid, owner);
    }

    #[test]
    fn rank_top_n_truncates_to_n() {
        let result = rank_top_n(
            vec![
                rank_entry(1, 10),
                rank_entry(2, 30),
                rank_entry(3, 20),
                rank_entry(4, 40),
            ],
            2,
        );
        assert_eq!(result, vec![4, 2]);
    }

    #[test]
    fn rank_top_n_zero_returns_empty() {
        let result = rank_top_n(vec![rank_entry(1, 10), rank_entry(2, 30)], 0);
        assert!(result.is_empty());
    }

    #[test]
    fn rank_top_n_larger_than_input_returns_all_sorted() {
        let result = rank_top_n(vec![rank_entry(1, 10), rank_entry(2, 30)], 100);
        assert_eq!(result, vec![2, 1]);
    }

    #[test]
    fn rank_top_n_empty_input_returns_empty() {
        let result = rank_top_n::<u32, U64F64>(vec![], 5);
        assert!(result.is_empty());
    }

    #[test]
    fn rank_top_n_ties_preserve_insertion_order() {
        let result = rank_top_n(
            vec![rank_entry(1, 10), rank_entry(2, 10), rank_entry(3, 10)],
            2,
        );
        assert_eq!(result, vec![1, 2]);
    }

    #[test]
    fn merge_inserts_first_observation() {
        let mut entries: Vec<(u32, I96F32)> = Vec::new();
        merge_owner_by_highest_price(&mut entries, 7, price(100));
        assert_eq!(entries, vec![(7, price(100))]);
    }

    #[test]
    fn merge_upgrades_to_higher_price_for_same_owner() {
        let mut entries = vec![(7, price(100))];
        merge_owner_by_highest_price(&mut entries, 7, price(250));
        assert_eq!(entries, vec![(7, price(250))]);
    }

    #[test]
    fn merge_keeps_existing_when_new_price_lower() {
        let mut entries = vec![(7, price(250))];
        merge_owner_by_highest_price(&mut entries, 7, price(100));
        assert_eq!(entries, vec![(7, price(250))]);
    }

    #[test]
    fn merge_keeps_one_entry_with_highest_price_for_owner_with_multiple_subnets() {
        let mut entries: Vec<(u32, I96F32)> = Vec::new();
        merge_owner_by_highest_price(&mut entries, 7, price(100));
        merge_owner_by_highest_price(&mut entries, 8, price(200));
        merge_owner_by_highest_price(&mut entries, 7, price(300));
        assert_eq!(entries, vec![(7, price(300)), (8, price(200))]);
    }

    #[test]
    fn top_validators_rank_by_ema_after_sample_threshold() {
        new_test_ext().execute_with(|| {
            let exact_threshold = account(1);
            let above_threshold = account(2);
            let below_threshold = account(3);
            set_members(
                CollectiveId::EconomicEligible,
                vec![
                    exact_threshold.clone(),
                    above_threshold.clone(),
                    below_threshold.clone(),
                ],
            );
            set_ema(&exact_threshold, 100, ECONOMIC_ELIGIBILITY_THRESHOLD);
            set_ema(
                &above_threshold,
                50,
                ECONOMIC_ELIGIBILITY_THRESHOLD.saturating_add(1),
            );
            set_ema(
                &below_threshold,
                1_000,
                ECONOMIC_ELIGIBILITY_THRESHOLD.saturating_sub(1),
            );

            let (members, weight) = TermManagement::top_validators(2);

            assert_eq!(members, vec![exact_threshold, above_threshold]);
            assert!(weight.ref_time() > 0);
        });
    }

    #[test]
    fn top_validators_returns_empty_when_no_candidate_has_enough_samples() {
        new_test_ext().execute_with(|| {
            let coldkey = account(1);
            set_members(CollectiveId::EconomicEligible, vec![coldkey.clone()]);
            set_ema(
                &coldkey,
                1_000,
                ECONOMIC_ELIGIBILITY_THRESHOLD.saturating_sub(1),
            );

            let (members, _) = TermManagement::top_validators(ECONOMIC_SIZE);

            assert!(members.is_empty());
        });
    }

    #[test]
    fn top_validators_zero_limit_returns_empty() {
        new_test_ext().execute_with(|| {
            let coldkey = account(1);
            set_members(CollectiveId::EconomicEligible, vec![coldkey.clone()]);
            set_ema(&coldkey, 1_000, ECONOMIC_ELIGIBILITY_THRESHOLD);

            let (members, _) = TermManagement::top_validators(0);

            assert!(members.is_empty());
        });
    }

    #[test]
    fn rotate_economic_keeps_old_members_when_validator_set_is_underfilled() {
        new_test_ext().execute_with(|| {
            let old_members = accounts(10, ECONOMIC_SIZE);
            let candidate = account(1);
            set_members(CollectiveId::Economic, old_members.clone());
            set_members(CollectiveId::EconomicEligible, vec![candidate.clone()]);
            set_ema(&candidate, 1_000, ECONOMIC_ELIGIBILITY_THRESHOLD);

            let weight = TermManagement::rotate_economic();

            assert!(weight.ref_time() > 0);
            assert_eq!(members_of(CollectiveId::Economic), old_members);
        });
    }

    #[test]
    fn top_subnet_owners_ranks_best_mature_subnet_per_owner() {
        new_test_ext().execute_with(|| {
            crate::System::set_block_number(1_000);
            let owner_a = account(1);
            let owner_b = account(2);
            let immature_owner = account(3);

            seed_subnet(NetUid::from(1_000), owner_a.clone(), 10, 700);
            seed_subnet(NetUid::from(1_001), owner_a.clone(), 30, 800);
            seed_subnet(NetUid::from(1_002), owner_b.clone(), 20, 750);
            seed_subnet(NetUid::from(1_003), immature_owner, 100, 950);

            let (members, weight) = TermManagement::top_subnet_owners(2, 100);

            assert_eq!(members, vec![owner_a, owner_b]);
            assert!(weight.ref_time() > 0);
        });
    }

    #[test]
    fn rotate_building_keeps_old_members_when_owner_set_is_underfilled() {
        new_test_ext().execute_with(|| {
            crate::System::set_block_number(1_000);
            let old_members = accounts(20, BUILDING_SIZE);
            let candidate = account(1);
            set_members(CollectiveId::Building, old_members.clone());
            seed_subnet(NetUid::from(1_000), candidate, 10, 0);

            let weight = TermManagement::rotate_building();

            assert!(weight.ref_time() > 0);
            assert_eq!(members_of(CollectiveId::Building), old_members);
        });
    }

    #[test]
    fn top_subnet_owners_includes_exact_min_age_boundary() {
        new_test_ext().execute_with(|| {
            crate::System::set_block_number(1_000);
            let exact_age_owner = account(1);
            let too_young_owner = account(2);

            seed_subnet(NetUid::from(1_000), exact_age_owner.clone(), 10, 900);
            seed_subnet(NetUid::from(1_001), too_young_owner, 100, 901);

            let (members, _) = TermManagement::top_subnet_owners(1, 100);

            assert_eq!(members, vec![exact_age_owner]);
        });
    }

    #[test]
    fn top_subnet_owners_zero_limit_returns_empty() {
        new_test_ext().execute_with(|| {
            crate::System::set_block_number(1_000);
            seed_subnet(NetUid::from(1_000), account(1), 10, 0);

            let (members, _) = TermManagement::top_subnet_owners(0, 100);

            assert!(members.is_empty());
        });
    }
}
