//! Benchmarking setup
#![cfg(feature = "runtime-benchmarks")]
#![allow(clippy::arithmetic_side_effects)]
#![allow(clippy::unwrap_used)]

extern crate alloc;
use alloc::vec::Vec;

#[allow(unused)]
use crate::Pallet as AdminUtils;
use frame_benchmarking::v1::account;
use frame_benchmarking::v2::*;
use frame_support::{BoundedVec, assert_noop, dispatch::UnfilteredDispatchable};
use frame_system::RawOrigin;
use pallet_subtensor::SubnetworkN;
use scale_info::prelude::vec;
use sp_runtime::traits::Get;
use subtensor_runtime_common::NetUid;

use super::*;

/// Seed the complete shared Null capacity, including historical dense rows,
/// frozen Yuma bonds, EVM associations and the bounded commit-cleanup budget.
fn setup_null_pruning_benchmark<T: Config>(count: u8) -> NetUid {
    use alloc::collections::VecDeque;
    use pallet_subtensor::*;
    use sp_runtime::PerU16;
    use subtensor_runtime_common::{AlphaBalance, NetUidStorageIndex};

    let netuid = NetUid::from(1);
    let n = subnets::mechanism::NULL_UID_BUDGET / u16::from(count);
    let len = usize::from(n);
    Pallet::<T>::set_admin_freeze_window(0);
    Pallet::<T>::init_new_network(netuid, u16::MAX - 1);
    Pallet::<T>::set_epoch_consensus(netuid, EpochConsensus::Null);
    Pallet::<T>::set_max_allowed_uids(netuid, n);
    Pallet::<T>::set_max_allowed_validators(netuid, 1);
    Pallet::<T>::set_immunity_period(netuid, 0);
    StakeThreshold::<T>::put(0u64);
    MechanismCountCurrent::<T>::insert(netuid, subtensor_runtime_common::MechId::from(count));
    SubnetworkN::<T>::insert(netuid, n);
    Active::<T>::insert(netuid, vec![true; len]);
    Emission::<T>::insert(
        netuid,
        (0..n)
            .map(|uid| AlphaBalance::from(u64::from(uid)))
            .collect::<Vec<_>>(),
    );
    Consensus::<T>::insert(netuid, vec![PerU16::zero(); len]);
    Dividends::<T>::insert(netuid, vec![PerU16::zero(); len]);
    ValidatorTrust::<T>::insert(netuid, vec![PerU16::zero(); len]);
    StakeWeight::<T>::insert(netuid, vec![0u16; len]);
    ValidatorPermit::<T>::insert(netuid, (0..n).map(|uid| uid == n - 1).collect::<Vec<_>>());
    let row = (0..n).map(|uid| (uid, u16::MAX)).collect::<Vec<_>>();
    for mechanism in 0..count {
        let index = Pallet::<T>::get_mechanism_storage_index(netuid, mechanism.into());
        Incentive::<T>::insert(index, vec![PerU16::zero(); len]);
        ConsensusByMechanism::<T>::insert(index, vec![PerU16::zero(); len]);
        LastUpdate::<T>::insert(index, vec![0u64; len]);
        for uid in 0..n {
            Weights::<T>::insert(index, uid, &row);
        }
    }
    let frozen_n = DefaultMaxAllowedUids::<T>::get().min(n);
    let frozen_row = (0..frozen_n).map(|uid| (uid, u16::MAX)).collect::<Vec<_>>();
    for uid in 0..frozen_n {
        Bonds::<T>::insert(NetUidStorageIndex::from(netuid), uid, &frozen_row);
    }
    for uid in 0..n {
        let hotkey: T::AccountId = account("null_trim_hotkey", u32::from(uid), 0);
        let coldkey: T::AccountId = account("null_trim_coldkey", u32::from(uid), 0);
        Owner::<T>::insert(&hotkey, &coldkey);
        Keys::<T>::insert(netuid, uid, &hotkey);
        Uids::<T>::insert(netuid, &hotkey, uid);
        BlockAtRegistration::<T>::insert(netuid, uid, 0);
        IsNetworkMember::<T>::insert(&hotkey, netuid, true);
        Pallet::<T>::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            netuid,
            AlphaBalance::from(if uid == n - 1 {
                3_000_000_000u64
            } else {
                1_000_000_000u64
            }),
        );
        let mut evm_address = [0u8; 20];
        for (destination, source) in evm_address
            .iter_mut()
            .skip(12)
            .zip((u64::from(uid) + 1).to_be_bytes())
        {
            *destination = source;
        }
        Pallet::<T>::set_associated_evm_address(netuid, uid, evm_address.into(), 0);
        let children = (1..=5u16)
            .map(|offset| {
                let child: T::AccountId =
                    account("null_trim_hotkey", u32::from((uid + offset) % n), 0);
                (u64::MAX / 10, child)
            })
            .collect::<Vec<_>>();
        let parents = (1..=5u16)
            .map(|offset| {
                let parent: T::AccountId =
                    account("null_trim_hotkey", u32::from((uid + n - offset) % n), 0);
                (u64::MAX / 10, parent)
            })
            .collect::<Vec<_>>();
        ChildKeys::<T>::insert(&hotkey, netuid, children);
        ParentKeys::<T>::insert(&hotkey, netuid, parents);
    }
    // 4,095 legacy hash keys plus one timelock key exercise cleanup AND UID
    // compaction in the same bounded call; one extra key would defer compaction.
    let index = NetUidStorageIndex::from(netuid);
    for uid in 0..subnets::mechanism::NULL_UID_BUDGET - 1 {
        let hotkey: T::AccountId = account("null_trim_old_commit", u32::from(uid), 0);
        WeightCommits::<T>::insert(
            index,
            &hotkey,
            VecDeque::from([(Default::default(), 0, 1, 0)]),
        );
    }
    let winner: T::AccountId = account("null_trim_hotkey", u32::from(n - 1), 0);
    TimelockedWeightCommits::<T>::mutate(index, 0, |queue| {
        queue.push_back((winner, 0, vec![1u8].try_into().unwrap(), 1000));
    });
    frame_system::Pallet::<T>::set_block_number(10u32.into());
    netuid
}

#[benchmarks]
mod benchmarks {
    use super::*;
    #[cfg(test)]
    use crate::tests::mock;
    use sp_runtime::PerU16;
    use substrate_fixed::types::{I64F64, U64F64};
    use subtensor_runtime_common::{NetUid, TaoBalance};

    #[benchmark]
    fn swap_authorities(a: Linear<0, 32>) {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);

        let mut value: BoundedVec<
            <T as pallet::Config>::AuthorityId,
            <T as pallet::Config>::MaxAuthorities,
        > = BoundedVec::new();

        for idx in 1..=a {
            let authority: <T as pallet::Config>::AuthorityId = account("Authority", idx, 0u32);
            let result = value.try_push(authority.clone());
            if result.is_err() {
                // Handle the error, perhaps by breaking the loop or logging an error message
            }
        }

        #[extrinsic_call]
        _(RawOrigin::Root, value);
    }

    #[benchmark]
    fn schedule_grandpa_change(a: Linear<0, 32>) {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        let next_authorities = (1..=a)
            .map(|idx| account("Authority", idx, 0u32))
            .collect::<Vec<(sp_consensus_grandpa::AuthorityId, u64)>>();
        let in_blocks = BlockNumberFor::<T>::from(0u32);

        #[extrinsic_call]
        _(RawOrigin::Root, next_authorities, in_blocks, None);
    }

    #[benchmark]
    fn sudo_set_default_take() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        #[extrinsic_call]
        _(RawOrigin::Root, PerU16::from_parts(100)/*default_take*/)/*sudo_set_default_take*/;
    }

    #[benchmark]
    fn sudo_set_serving_rate_limit() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        #[extrinsic_call]
		_(RawOrigin::Root, 1u16.into()/*netuid*/, 100u64/*serving_rate_limit*/)/*sudo_set_serving_rate_limit*/;
    }

    #[benchmark]
    fn sudo_set_max_difficulty() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /*netuid*/
            1u16,        /*tempo*/
        );

        #[extrinsic_call]
		_(RawOrigin::Root, 1u16.into()/*netuid*/, 10000u64/*max_difficulty*/)/*sudo_set_max_difficulty*/;
    }

    #[benchmark]
    fn sudo_set_min_difficulty() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /*netuid*/
            1u16,        /*tempo*/
        );

        #[extrinsic_call]
		_(RawOrigin::Root, 1u16.into()/*netuid*/, 1000u64/*min_difficulty*/)/*sudo_set_min_difficulty*/;
    }

    #[benchmark]
    fn sudo_set_weights_set_rate_limit() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /*netuid*/
            1u16,        /*tempo*/
        );

        #[extrinsic_call]
		_(RawOrigin::Root, 1u16.into()/*netuid*/, 3u64/*rate_limit*/)/*sudo_set_weights_set_rate_limit*/;
    }

    #[benchmark]
    fn sudo_set_weights_version_key() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /*netuid*/
            1u16,        /*tempo*/
        );

        #[extrinsic_call]
		_(RawOrigin::Root, 1u16.into()/*netuid*/, 1u64/*version_key*/)/*sudo_set_weights_version_key*/;
    }

    #[benchmark]
    fn sudo_set_bonds_moving_average() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /*netuid*/
            1u16,        /*tempo*/
        );

        #[extrinsic_call]
		_(RawOrigin::Root, 1u16.into()/*netuid*/, 100u64/*bonds_moving_average*/)/*sudo_set_bonds_moving_average*/;
    }

    #[benchmark]
    fn sudo_set_bonds_penalty() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /*netuid*/
            1u16,        /*tempo*/
        );

        #[extrinsic_call]
        _(RawOrigin::Root, 1u16.into()/*netuid*/, 100u16/*bonds_penalty*/)/*sudo_set_bonds_penalty*/;
    }

    #[benchmark]
    fn sudo_set_max_allowed_validators() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /*netuid*/
            1u16,        /*tempo*/
        );

        #[extrinsic_call]
		_(RawOrigin::Root, 1u16.into()/*netuid*/, 10u16/*max_allowed_validators*/)/*sudo_set_max_allowed_validators*/;
    }

    #[benchmark]
    fn sudo_set_difficulty() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /*netuid*/
            1u16,        /*tempo*/
        );

        #[extrinsic_call]
		_(RawOrigin::Root, 1u16.into()/*netuid*/, 1200000u64/*difficulty*/)/*sudo_set_difficulty*/;
    }

    #[benchmark]
    fn sudo_set_adjustment_interval() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /*netuid*/
            1u16,        /*tempo*/
        );

        #[extrinsic_call]
		_(RawOrigin::Root, 1u16.into()/*netuid*/, 12u16/*adjustment_interval*/)/*sudo_set_adjustment_interval*/;
    }

    #[benchmark]
    fn sudo_set_target_registrations_per_interval() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /*netuid*/
            1u16,        /*tempo*/
        );

        #[extrinsic_call]
		_(RawOrigin::Root, 1u16.into()/*netuid*/, 300u16/*target_registrations*/)/*sudo_set_target_registrations_per_interval*/;
    }

    #[benchmark]
    fn sudo_set_activity_cutoff() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /*netuid*/
            1u16,        /*tempo*/
        );

        #[extrinsic_call]
		_(RawOrigin::Root, 1u16.into()/*netuid*/, 361u16/*activity_cutoff*/)/*sudo_set_activity_cutoff*/;
    }

    #[benchmark]
    fn sudo_set_activity_cutoff_factor() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /*netuid*/
            1u16,        /*tempo*/
        );

        #[extrinsic_call]
		_(RawOrigin::Root, 1u16.into()/*netuid*/, 5_000u32/*factor_milli*/)/*sudo_set_activity_cutoff_factor*/;
    }

    #[benchmark]
    fn sudo_set_rho() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /*netuid*/
            1u16,        /*tempo*/
        );

        #[extrinsic_call]
		_(RawOrigin::Root, 1u16.into()/*netuid*/, 300u16/*rho*/)/*sudo_set_rho*/;
    }

    #[benchmark]
    fn sudo_set_kappa() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /*netuid*/
            1u16,        /*sudo_tempo*/
        );

        #[extrinsic_call]
		_(RawOrigin::Root, 1u16.into()/*netuid*/, 3u16/*kappa*/)/*set_kappa*/;
    }

    #[benchmark]
    fn sudo_set_min_allowed_uids() {
        let netuid = NetUid::from(1);
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(netuid, 1u16 /*tempo*/);

        // Artificially set that some neurons are already registered
        SubnetworkN::<T>::set(netuid, 32);

        #[extrinsic_call]
		_(RawOrigin::Root, netuid, 16u16/*min_allowed_uids*/)/*sudo_set_min_allowed_uids*/;
    }

    #[benchmark]
    fn sudo_set_max_allowed_uids() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /*netuid*/
            1u16,        /*tempo*/
        );

        #[extrinsic_call]
		_(RawOrigin::Root, 1u16.into()/*netuid*/, 256u16/*max_allowed_uids*/)/*sudo_set_max_allowed_uids*/;
    }

    #[benchmark]
    fn sudo_set_min_allowed_weights() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /*netuid*/
            1u16,        /*tempo*/
        );

        #[extrinsic_call]
		_(RawOrigin::Root, 1u16.into()/*netuid*/, 10u16/*max_allowed_uids*/)/*sudo_set_min_allowed_weights*/;
    }

    #[benchmark]
    fn sudo_set_immunity_period() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /*netuid*/
            1u16,        /*tempo*/
        );

        #[extrinsic_call]
		_(RawOrigin::Root, 1u16.into()/*netuid*/, 100u16/*immunity_period*/)/*sudo_set_immunity_period*/;
    }

    #[benchmark]
    fn sudo_set_max_registrations_per_block() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /*netuid*/
            1u16,        /*tempo*/
        );

        #[extrinsic_call]
		_(RawOrigin::Root, 1u16.into()/*netuid*/, 100u16/*max_registrations*/)/*sudo_set_max_registrations_per_block*/;
    }

    #[benchmark]
    fn sudo_set_max_burn() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /*netuid*/
            1u16,        /*tempo*/
        );

        #[extrinsic_call]
		_(RawOrigin::Root, 1u16.into()/*netuid*/, 2_000_000_000.into()/*max_burn*/)/*sudo_set_max_burn*/;
    }

    #[benchmark]
    fn sudo_set_min_burn() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /*netuid*/
            1u16,        /*tempo*/
        );

        #[extrinsic_call]
		_(RawOrigin::Root, 1u16.into()/*netuid*/, 10.into()/*min_burn*/)/*sudo_set_min_burn*/;
    }

    #[benchmark]
    fn sudo_set_network_registration_allowed() {
        let netuid = NetUid::from(1);
        let owner = setup_worst_case_admin_subnet::<T>(netuid);
        #[extrinsic_call]
        _(RawOrigin::Signed(owner), netuid, true);
        assert!(pallet_subtensor::NetworkRegistrationAllowed::<T>::get(
            netuid
        ));
    }

    #[benchmark]
    fn sudo_set_tempo() {
        let netuid = NetUid::from(1);
        let owner: T::AccountId = account("owner", 0, 0);

        // Benchmark the heavier owner path (bounds check + rate-limit read/write),
        // not the root path which bypasses both.
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(netuid, pallet_subtensor::MIN_TEMPO);
        pallet_subtensor::SubnetOwner::<T>::insert(netuid, &owner);

        #[extrinsic_call]
        _(
            RawOrigin::Signed(owner),
            netuid,
            pallet_subtensor::MIN_TEMPO + 1,
        );
    }

    #[benchmark]
    fn sudo_set_commit_reveal_weights_interval() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /*netuid*/
            1u16,        /*sudo_tempo*/
        );

        #[extrinsic_call]
		_(RawOrigin::Root, 1u16.into()/*netuid*/, 3u64/*interval*/)/*sudo_set_commit_reveal_weights_interval()*/;
    }

    #[benchmark]
    fn sudo_set_commit_reveal_weights_enabled() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /*netuid*/
            1u16,        /*sudo_tempo*/
        );

        #[extrinsic_call]
		_(RawOrigin::Root, 1u16.into()/*netuid*/, true/*enabled*/)/*set_commit_reveal_weights_enabled*/;
    }

    #[benchmark]
    fn sudo_set_commit_reveal_version() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /*netuid*/
            1u16,        /*sudo_tempo*/
        );

        #[extrinsic_call]
		_(RawOrigin::Root, 5u16/*version*/)/*sudo_set_commit_reveal_version()*/;
    }

    #[benchmark]
    fn sudo_set_tx_rate_limit() {
        #[extrinsic_call]
        _(RawOrigin::Root, 100u64);
    }

    #[benchmark]
    fn sudo_set_total_issuance() {
        let call = Call::<T>::sudo_set_total_issuance {
            total_issuance: 100u64.into(),
        };

        #[block]
        {
            assert_noop!(
                call.dispatch_bypass_filter(RawOrigin::Root.into()),
                Error::<T>::Deprecated
            );
        }
    }

    #[benchmark]
    fn sudo_set_rao_recycled() {
        let netuid = NetUid::from(1);
        pallet_subtensor::Pallet::<T>::init_new_network(
            netuid, 1u16, // tempo
        );

        #[extrinsic_call]
        _(RawOrigin::Root, netuid, 100u64.into());
    }

    #[benchmark]
    fn sudo_set_stake_threshold() {
        #[extrinsic_call]
        _(RawOrigin::Root, 100u64);
    }

    #[benchmark]
    fn sudo_set_nominator_min_required_stake() {
        #[extrinsic_call]
        _(RawOrigin::Root, 100u64);
    }

    #[benchmark]
    fn sudo_set_tx_delegate_take_rate_limit() {
        #[extrinsic_call]
        _(RawOrigin::Root, 100u64);
    }

    #[benchmark]
    fn sudo_set_min_delegate_take() {
        #[extrinsic_call]
        _(RawOrigin::Root, PerU16::from_parts(100));
    }

    #[benchmark]
    fn sudo_set_min_childkey_take_per_subnet() {
        let netuid = NetUid::from(1);
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            netuid, 1u16, // tempo
        );
        let take = PerU16::from_parts(pallet_subtensor::Pallet::<T>::get_max_childkey_take() / 2);

        #[extrinsic_call]
        _(RawOrigin::Root, netuid, take);
    }

    #[benchmark]
    fn sudo_set_liquid_alpha_enabled() {
        let netuid = NetUid::from(1);
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            netuid, 1u16, // tempo
        );

        #[extrinsic_call]
        _(RawOrigin::Root, netuid, true);
    }

    #[benchmark]
    fn sudo_set_alpha_values() {
        let netuid = NetUid::from(1);
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            netuid, 1u16, // tempo
        );
        pallet_subtensor::Pallet::<T>::set_liquid_alpha_enabled(netuid, true);

        #[extrinsic_call]
        _(RawOrigin::Root, netuid, 2000u16, 3000u16);
    }

    #[benchmark]
    fn sudo_set_liquid_alpha_consensus_mode() {
        let netuid = NetUid::from(1);
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(netuid, 1u16);

        #[extrinsic_call]
        _(
            RawOrigin::Root,
            netuid,
            pallet_subtensor::ConsensusMode::Previous,
        );
    }

    #[benchmark]
    fn sudo_set_epoch_consensus() {
        // Legacy state can exceed today's Yuma capacity. Charge the population accepted
        // by the target Null mode rather than assuming the default Yuma limit.
        let netuid = setup_null_pruning_benchmark::<T>(1);
        pallet_subtensor::Pallet::<T>::set_epoch_consensus(
            netuid,
            pallet_subtensor::EpochConsensus::Yuma,
        );
        pallet_subtensor::Pallet::<T>::set_max_allowed_validators(netuid, 128);
        let index = pallet_subtensor::Pallet::<T>::get_mechanism_storage_index(netuid, 0.into());
        let _ = pallet_subtensor::TimelockedWeightCommits::<T>::clear_prefix(index, u32::MAX, None);
        #[extrinsic_call]
        _(
            RawOrigin::Root,
            netuid,
            pallet_subtensor::EpochConsensus::Null,
        );

        assert_eq!(
            pallet_subtensor::Pallet::<T>::get_epoch_consensus(netuid),
            pallet_subtensor::EpochConsensus::Null
        );
        assert_eq!(
            pallet_subtensor::ValidatorPermit::<T>::get(netuid)
                .iter()
                .filter(|permit| **permit)
                .count(),
            1
        );
        assert_eq!(
            pallet_subtensor::Pallet::<T>::get_max_allowed_validators(netuid),
            1
        );
    }

    #[benchmark]
    fn sudo_set_coldkey_swap_announcement_delay() {
        #[extrinsic_call]
        _(RawOrigin::Root, 100u32.into());
    }

    #[benchmark]
    fn sudo_set_coldkey_swap_reannouncement_delay() {
        #[extrinsic_call]
        _(RawOrigin::Root, 100u32.into());
    }

    #[benchmark]
    fn sudo_set_dissolve_network_schedule_duration() {
        #[extrinsic_call]
        _(RawOrigin::Root, 100u32.into());
    }

    #[benchmark]
    fn sudo_set_toggle_transfer() {
        let netuid = NetUid::from(1);
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            netuid, 1u16, // tempo
        );

        #[extrinsic_call]
        _(RawOrigin::Root, netuid, true);
    }

    #[benchmark]
    fn sudo_toggle_evm_precompile() {
        #[extrinsic_call]
        _(RawOrigin::Root, PrecompileEnum::Staking, true);
    }

    #[benchmark]
    fn sudo_set_subnet_moving_alpha() {
        #[extrinsic_call]
        _(RawOrigin::Root, 100u64.into());
    }

    #[benchmark]
    fn sudo_set_ema_price_halving_period() {
        let netuid = NetUid::from(1);

        pallet_subtensor::Pallet::<T>::init_new_network(
            netuid, 1u16, // tempo
        );

        #[extrinsic_call]
        _(RawOrigin::Root, netuid, 100u64);
    }

    #[benchmark]
    fn sudo_set_alpha_sigmoid_steepness() {
        let netuid = NetUid::from(1);
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            netuid, 1u16, // tempo
        );

        #[extrinsic_call]
        _(RawOrigin::Root, netuid, 100i16);
    }

    #[benchmark]
    fn sudo_set_yuma3_enabled() {
        let netuid = NetUid::from(1);
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            netuid, 1u16, // tempo
        );

        #[extrinsic_call]
        _(RawOrigin::Root, netuid, true);
    }

    #[benchmark]
    fn sudo_set_bonds_reset_enabled() {
        let netuid = NetUid::from(1);
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            netuid, 1u16, // tempo
        );

        #[extrinsic_call]
        _(RawOrigin::Root, netuid, true);
    }

    #[benchmark]
    fn sudo_set_subnet_emission_enabled() {
        let netuid = NetUid::from(1);
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            netuid, 1u16, // tempo
        );

        #[extrinsic_call]
        _(RawOrigin::Root, netuid, false);

        assert!(!pallet_subtensor::SubnetEmissionEnabled::<T>::get(netuid));
    }
    #[benchmark]
    fn sudo_set_sn_owner_hotkey() {
        let netuid = NetUid::from(1);
        let hotkey: T::AccountId = account("Alice", 0, 1);

        pallet_subtensor::Pallet::<T>::init_new_network(
            netuid, 1u16, // tempo
        );

        #[extrinsic_call]
        _(RawOrigin::Root, netuid, hotkey);
    }

    #[benchmark]
    fn sudo_set_subtoken_enabled() {
        let netuid = NetUid::from(1);
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            netuid, 1u16, // tempo
        );

        #[extrinsic_call]
        _(RawOrigin::Root, netuid, true);
    }

    #[benchmark]
    fn sudo_set_admin_freeze_window() {
        #[extrinsic_call]
		_(RawOrigin::Root, 5u16/*window*/)/*sudo_set_admin_freeze_window*/;
    }

    #[benchmark]
    fn sudo_set_owner_hparam_rate_limit() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        #[extrinsic_call]
		_(RawOrigin::Root, 2u16/*epochs*/)/*sudo_set_owner_hparam_rate_limit*/;
    }

    #[benchmark]
    fn sudo_set_owner_immune_neuron_limit() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /*netuid*/
            1u16,        /*sudo_tempo*/
        );

        #[extrinsic_call]
        _(RawOrigin::Root, 1u16.into()/*netuid*/, 5u16/*immune_neurons*/)/*sudo_set_owner_immune_neuron_limit()*/;
    }

    #[benchmark]
    fn sudo_trim_to_max_allowed_uids() {
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /*netuid*/
            1u16,        /*sudo_tempo*/
        );

        // Include actual deletion and UID compaction at the full Yuma limit;
        // an empty subnet only measures changing MaxAllowedUids.
        let netuid = NetUid::from(1);
        let n = pallet_subtensor::DefaultMaxAllowedUids::<T>::get();
        pallet_subtensor::Pallet::<T>::set_epoch_consensus(
            netuid,
            pallet_subtensor::EpochConsensus::Yuma,
        );
        pallet_subtensor::Pallet::<T>::set_max_allowed_uids(netuid, n);
        pallet_subtensor::Pallet::<T>::set_immunity_period(netuid, 0);
        for uid in 0..n {
            let hotkey: T::AccountId = account("trim_hotkey", u32::from(uid), 0);
            let coldkey: T::AccountId = account("trim_coldkey", u32::from(uid), 0);
            pallet_subtensor::Owner::<T>::insert(&hotkey, &coldkey);
            pallet_subtensor::Pallet::<T>::append_neuron(netuid, &hotkey, 0);
            // Keep dense rows on the retained high-emission neurons so target
            // filtering and remapping are exercised as well as UID deletion.
            if uid >= n.saturating_sub(64) {
                pallet_subtensor::Weights::<T>::insert(
                    subtensor_runtime_common::NetUidStorageIndex::from(netuid),
                    uid,
                    (0..n).map(|target| (target, u16::MAX)).collect::<Vec<_>>(),
                );
            }
        }
        pallet_subtensor::Emission::<T>::insert(
            netuid,
            (0..n)
                .map(|uid| subtensor_runtime_common::AlphaBalance::from(u64::from(uid)))
                .collect::<Vec<_>>(),
        );

        #[extrinsic_call]
		_(RawOrigin::Root, 1u16.into()/*netuid*/, 64u16/*max_n*/)/*sudo_trim_to_max_allowed_uids()*/;

        assert_eq!(SubnetworkN::<T>::get(netuid), 64);
    }

    #[benchmark]
    fn sudo_trim_null_uids_batch() {
        let netuid = setup_null_pruning_benchmark::<T>(1);
        let before = SubnetworkN::<T>::get(netuid);

        #[extrinsic_call]
        _(RawOrigin::Root, netuid, 256u16);

        assert_eq!(
            SubnetworkN::<T>::get(netuid),
            before - pallet_subtensor::subnets::uids::NULL_PRUNING_BATCH
        );
        assert_eq!(
            pallet_subtensor::NullPruningTarget::<T>::get(netuid),
            Some(256)
        );
    }

    #[benchmark]
    fn sudo_trim_null_uids_batch_many_mechanisms() {
        let netuid = setup_null_pruning_benchmark::<T>(16);
        let before = SubnetworkN::<T>::get(netuid);

        #[block]
        {
            assert!(
                AdminUtils::<T>::sudo_trim_null_uids_batch(RawOrigin::Root.into(), netuid, 64)
                    .is_ok()
            );
        }
        assert_eq!(
            SubnetworkN::<T>::get(netuid),
            before - pallet_subtensor::subnets::uids::NULL_PRUNING_BATCH
        );
        assert_eq!(
            pallet_subtensor::NullPruningTarget::<T>::get(netuid),
            Some(64)
        );
    }

    #[benchmark]
    fn sudo_set_min_non_immune_uids() {
        // disable admin freeze window
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        // create a network for netuid = 1
        pallet_subtensor::Pallet::<T>::init_new_network(
            1u16.into(), /* netuid */
            1u16,        /* sudo_tempo */
        );

        #[extrinsic_call]
        _(
            RawOrigin::Root,
            1u16.into(), /* netuid */
            12u16,       /* min */
        ); /* sudo_set_min_non_immune_uids() */
    }

    #[benchmark]
    fn sudo_set_max_epochs_per_block() {
        #[extrinsic_call]
        _(RawOrigin::Root, 8u8);

        assert_eq!(
            pallet_subtensor::Pallet::<T>::get_max_epochs_per_block(),
            8u8
        );
    }

    fn setup_worst_case_admin_subnet<T: Config>(netuid: NetUid) -> T::AccountId {
        let owner: T::AccountId = whitelisted_caller();
        pallet_subtensor::Pallet::<T>::set_admin_freeze_window(0);
        pallet_subtensor::Pallet::<T>::init_new_network(netuid, 1u16);
        pallet_subtensor::Pallet::<T>::set_max_allowed_uids(netuid, 1);
        pallet_subtensor::SubnetOwner::<T>::insert(netuid, owner.clone());
        owner
    }

    fn max_emission_split() -> Vec<u16> {
        let mut split = vec![4096u16; 15];
        split.push(4095u16);
        split
    }

    #[benchmark]
    fn sudo_set_adjustment_alpha() {
        let netuid = NetUid::from(1);
        let owner = setup_worst_case_admin_subnet::<T>(netuid);

        #[extrinsic_call]
        _(RawOrigin::Signed(owner), netuid, u64::MAX);
    }

    #[benchmark]
    fn sudo_set_network_pow_registration_allowed() {
        let netuid = NetUid::from(1);
        let owner = setup_worst_case_admin_subnet::<T>(netuid);
        #[extrinsic_call]
        _(RawOrigin::Signed(owner), netuid, true);
        assert!(pallet_subtensor::NetworkPowRegistrationAllowed::<T>::get(
            netuid
        ));
    }

    #[benchmark]
    fn sudo_set_subnet_owner_cut() {
        #[extrinsic_call]
        _(RawOrigin::Root, u16::MAX);
    }

    #[benchmark]
    fn sudo_set_network_rate_limit() {
        #[extrinsic_call]
        _(RawOrigin::Root, u64::MAX);
    }

    #[benchmark]
    fn sudo_set_network_immunity_period() {
        #[extrinsic_call]
        _(RawOrigin::Root, u64::MAX);
    }

    #[benchmark]
    fn sudo_set_network_min_lock_cost() {
        #[extrinsic_call]
        _(RawOrigin::Root, TaoBalance::from(u64::MAX));
    }

    #[benchmark]
    fn sudo_set_subnet_limit() {
        #[extrinsic_call]
        _(RawOrigin::Root, u16::MAX);
    }

    #[benchmark]
    fn sudo_set_lock_reduction_interval() {
        #[extrinsic_call]
        _(RawOrigin::Root, u64::MAX);
    }

    #[benchmark]
    fn sudo_set_evm_chain_id() {
        #[extrinsic_call]
        _(RawOrigin::Root, u64::MAX);
    }

    #[benchmark]
    fn sudo_set_recycle_or_burn() {
        let netuid = NetUid::from(1);
        let owner = setup_worst_case_admin_subnet::<T>(netuid);

        #[extrinsic_call]
        _(
            RawOrigin::Signed(owner),
            netuid,
            pallet_subtensor::RecycleOrBurnEnum::Burn,
        );
    }

    #[benchmark]
    fn sudo_set_ck_burn() {
        #[extrinsic_call]
        _(RawOrigin::Root, u64::MAX);
    }
    #[benchmark]
    fn sudo_set_mechanism_count() {
        // Removing 15 mechanisms includes the largest reachable historical
        // row prefixes, not only the cheap creation of empty mechanisms.
        let netuid = setup_null_pruning_benchmark::<T>(16);
        let owner: T::AccountId = whitelisted_caller();
        pallet_subtensor::SubnetOwner::<T>::insert(netuid, &owner);
        for mechanism in 0..16u8 {
            let index = pallet_subtensor::Pallet::<T>::get_mechanism_storage_index(
                netuid,
                mechanism.into(),
            );
            let _ = pallet_subtensor::WeightCommits::<T>::clear_prefix(index, u32::MAX, None);
            let _ =
                pallet_subtensor::TimelockedWeightCommits::<T>::clear_prefix(index, u32::MAX, None);
        }
        // Distribute the maximum frozen Yuma bond rows among removed mechanisms.
        let _ = pallet_subtensor::Bonds::<T>::clear_prefix(
            pallet_subtensor::Pallet::<T>::get_mechanism_storage_index(netuid, 0.into()),
            u32::MAX,
            None,
        );
        // A previous 16-mechanism Yuma subnet had at most 16 populated UIDs
        // per mechanism. It could subsequently grow to 256 UIDs in Null.
        let bond_row = (0..16u16).map(|uid| (uid, u16::MAX)).collect::<Vec<_>>();
        for mechanism in 0..16u8 {
            let index = pallet_subtensor::Pallet::<T>::get_mechanism_storage_index(
                netuid,
                mechanism.into(),
            );
            for uid in 0..16u16 {
                pallet_subtensor::Bonds::<T>::insert(index, uid, &bond_row);
            }
        }
        let mechanism_count = 1u8.into();

        #[extrinsic_call]
        _(RawOrigin::Signed(owner), netuid, mechanism_count);

        assert_eq!(
            pallet_subtensor::MechanismCountCurrent::<T>::get(netuid),
            mechanism_count
        );
    }

    #[benchmark]
    fn sudo_set_mechanism_emission_split() {
        let netuid = NetUid::from(1);
        let owner = setup_worst_case_admin_subnet::<T>(netuid);
        let mechanism_count = 16u8.into();
        assert!(pallet_subtensor::Pallet::<T>::do_set_max_mechanism_count(mechanism_count).is_ok());
        assert!(
            pallet_subtensor::Pallet::<T>::do_set_mechanism_count(netuid, mechanism_count).is_ok()
        );

        #[extrinsic_call]
        _(RawOrigin::Signed(owner), netuid, Some(max_emission_split()));
    }

    #[benchmark]
    fn sudo_set_tao_flow_cutoff() {
        #[extrinsic_call]
        _(RawOrigin::Root, I64F64::from_num(i64::MAX));
    }

    #[benchmark]
    fn sudo_set_tao_flow_normalization_exponent() {
        #[extrinsic_call]
        _(RawOrigin::Root, U64F64::from_num(2));
    }

    #[benchmark]
    fn sudo_set_emission_bar_quantile() {
        #[extrinsic_call]
        _(RawOrigin::Root, U64F64::from_num(0.61));
    }

    #[benchmark]
    fn sudo_set_emission_bar_rank() {
        #[extrinsic_call]
        _(RawOrigin::Root, 64u16);
    }

    #[benchmark]
    fn sudo_set_emission_gate_exponent() {
        #[extrinsic_call]
        _(RawOrigin::Root, U64F64::from_num(3));
    }

    #[benchmark]
    fn sudo_set_tao_flow_smoothing_factor() {
        #[extrinsic_call]
        _(RawOrigin::Root, u64::MAX);
    }

    #[benchmark]
    fn sudo_set_net_tao_flow_enabled() {
        #[extrinsic_call]
        _(RawOrigin::Root, true);
    }

    #[benchmark]
    fn sudo_set_max_mechanism_count() {
        #[extrinsic_call]
        _(RawOrigin::Root, 16u8.into());
    }

    #[benchmark]
    fn sudo_set_start_call_delay() {
        #[extrinsic_call]
        _(RawOrigin::Root, u64::MAX);
    }

    #[benchmark]
    fn sudo_set_burn_half_life() {
        let netuid = NetUid::from(1);
        let owner = setup_worst_case_admin_subnet::<T>(netuid);
        let max_half_life = pallet_subtensor::MaxBurnHalfLife::<T>::get();

        #[extrinsic_call]
        _(RawOrigin::Signed(owner), netuid, max_half_life);
    }

    #[benchmark]
    fn sudo_set_burn_increase_mult() {
        let netuid = NetUid::from(1);
        let owner = setup_worst_case_admin_subnet::<T>(netuid);

        #[extrinsic_call]
        _(RawOrigin::Signed(owner), netuid, U64F64::from_num(3));
    }

    #[benchmark]
    fn sudo_set_collateral_lock_share() {
        let netuid = NetUid::from(1);
        let owner = setup_worst_case_admin_subnet::<T>(netuid);
        let max_share = pallet_subtensor::MaxCollateralLockShare::<T>::get();

        #[extrinsic_call]
        _(RawOrigin::Signed(owner), netuid, max_share);
    }

    #[benchmark]
    fn sudo_set_collateral_drain_ratio() {
        let netuid = NetUid::from(1);
        let owner = setup_worst_case_admin_subnet::<T>(netuid);

        #[extrinsic_call]
        _(RawOrigin::Signed(owner), netuid, U64F64::from_num(1));
    }

    #[benchmark]
    fn sudo_set_owner_cut_enabled() {
        let netuid = NetUid::from(1);
        let owner = setup_worst_case_admin_subnet::<T>(netuid);

        #[extrinsic_call]
        _(RawOrigin::Signed(owner), netuid, true);
    }

    #[benchmark]
    fn sudo_set_owner_cut_auto_lock_enabled() {
        let netuid = NetUid::from(1);
        let owner = setup_worst_case_admin_subnet::<T>(netuid);

        #[extrinsic_call]
        _(RawOrigin::Signed(owner), netuid, true);
    }

    #[benchmark]
    fn sudo_set_basket_concentration_cap() {
        #[extrinsic_call]
        _(RawOrigin::Root, 4096u16);
    }

    #[benchmark]
    fn sudo_set_basket_trading_enabled() {
        #[extrinsic_call]
        _(RawOrigin::Root, true);
    }

    #[benchmark]
    fn sudo_set_basket_trading_frozen() {
        let hotkey: T::AccountId = account("basket_frozen_hot", 0, 1);

        #[extrinsic_call]
        _(RawOrigin::Root, hotkey.clone(), true);

        assert!(pallet_subtensor::BasketTradingFrozen::<T>::contains_key(
            &hotkey
        ));
    }

    #[benchmark]
    fn sudo_set_basket_daily_turnover_cap() {
        #[extrinsic_call]
        _(RawOrigin::Root, 6553u16);
    }

    #[benchmark]
    fn sudo_set_basket_liquidity_cap() {
        #[extrinsic_call]
        _(RawOrigin::Root, 6553u16);
    }

    #[benchmark]
    fn sudo_set_basket_claim_dust() {
        #[extrinsic_call]
        _(
            RawOrigin::Root,
            1_000_000_000u64,
            10u16,
            100_000u64,
            10_000_000u64,
        );
    }

    impl_benchmark_test_suite!(AdminUtils, mock::new_test_ext(), mock::Test);
}
