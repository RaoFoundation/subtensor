#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use super::mock::*;
use crate::*;
use frame_support::{assert_noop, assert_ok};
use sp_core::U256;
use subtensor_runtime_common::{MechId, NetUidStorageIndex};

fn setup(n: u16) -> NetUid {
    let net = NetUid::from(1);
    System::set_block_number(1);
    SubtensorModule::init_new_network(net, 360);
    SubnetOwner::<Test>::insert(net, U256::from(100));
    SubnetOwnerHotkey::<Test>::insert(net, U256::from(0));
    SubtensorModule::append_neuron(net, &U256::from(0), 1);
    assert_ok!(SubtensorModule::enable_null_consensus(
        RuntimeOrigin::signed(U256::from(100)),
        net
    ));
    for uid in 1..n {
        SubtensorModule::append_neuron(net, &U256::from(uid), 1);
    }
    SubtensorModule::set_weights_set_rate_limit(net, 0);
    SubtensorModule::set_stake_threshold(0);
    System::set_block_number(2);
    net
}

#[test]
fn null_consensus_profile_is_opt_in_and_owner_gated() {
    new_test_ext(1).execute_with(|| {
        let net = setup(1);
        assert!(NullConsensus::<Test>::get(net));
        assert_eq!(MaxAllowedUids::<Test>::get(net), 32768);
        assert_eq!(Burn::<Test>::get(net), TaoBalance::ZERO);
        assert_eq!(CollateralLockShare::<Test>::get(net), 0);
        assert!(!CommitRevealWeightsEnabled::<Test>::get(net));
        assert!(!NullConsensus::<Test>::get(NetUid::from(2)));
        assert!(
            SubtensorModule::enable_null_consensus(RuntimeOrigin::signed(U256::from(101)), net)
                .is_err()
        );
        assert_noop!(
            SubtensorModule::burned_register(
                RuntimeOrigin::signed(U256::from(100)),
                net,
                U256::from(2)
            )
            .map_err(|e| e.error),
            Error::<Test>::NullConsensusRequiresPowRegistration
        );
        assert_noop!(
            SubtensorModule::do_set_mechanism_count(net, MechId::from(2)),
            Error::<Test>::NullConsensusRequiresSingleMechanism
        );
    });
}

#[test]
fn null_consensus_setup_errors_identify_the_failed_precondition() {
    new_test_ext(1).execute_with(|| {
        let net = setup(1);
        assert_noop!(
            SubtensorModule::enable_null_consensus(RuntimeOrigin::root(), net),
            Error::<Test>::NullConsensusAlreadyEnabled
        );
        NullConsensus::<Test>::remove(net);
        FirstEmissionBlockNumber::<Test>::insert(net, 1);
        assert_noop!(
            SubtensorModule::enable_null_consensus(RuntimeOrigin::root(), net),
            Error::<Test>::NullConsensusRequiresUnstartedSubnet
        );
        FirstEmissionBlockNumber::<Test>::remove(net);
        SubnetworkN::<Test>::insert(net, 2);
        assert_noop!(
            SubtensorModule::enable_null_consensus(RuntimeOrigin::root(), net),
            Error::<Test>::NullConsensusRequiresEmptySubnet
        );
        SubnetworkN::<Test>::insert(net, 1);
        MechanismCountCurrent::<Test>::insert(net, MechId::from(2));
        assert_noop!(
            SubtensorModule::enable_null_consensus(RuntimeOrigin::root(), net),
            Error::<Test>::NullConsensusRequiresSingleMechanism
        );
        MechanismCountCurrent::<Test>::insert(net, MechId::from(1));
        Weights::<Test>::insert(NetUidStorageIndex::from(net), 0, vec![(0, 1)]);
        assert_noop!(
            SubtensorModule::enable_null_consensus(RuntimeOrigin::root(), net),
            Error::<Test>::NullConsensusHasLegacyWeightsOrBonds
        );
        assert_noop!(
            SubtensorModule::set_weights_v2(
                RuntimeOrigin::signed(U256::zero()),
                net,
                vec![0],
                vec![1],
                0
            ),
            Error::<Test>::NullConsensusNotEnabled
        );
    });
}

#[test]
fn null_consensus_admission_errors_distinguish_required_work_and_paused_flags() {
    new_test_ext(1).execute_with(|| {
        let net = setup(1);
        assert_noop!(
            SubtensorModule::register_neuron(net, &U256::from(8)),
            Error::<Test>::NullConsensusRequiresPowRegistration
        );
        assert_noop!(
            SubtensorModule::trim_to_max_allowed_uids(net, 1),
            Error::<Test>::NullConsensusTrimmingDisabled
        );
        assert_noop!(
            SubtensorModule::set_weights(
                RuntimeOrigin::signed(U256::zero()),
                net,
                vec![0],
                vec![1],
                0
            ),
            Error::<Test>::NullConsensusRequiresU32Weights
        );
        let register = || {
            SubtensorModule::register(
                RuntimeOrigin::signed(U256::from(9)),
                net,
                1,
                0,
                vec![0; 32],
                U256::from(8),
                U256::from(9),
            )
        };
        NetworkRegistrationAllowed::<Test>::insert(net, false);
        assert_noop!(register(), Error::<Test>::NullConsensusRegistrationDisabled);
        NetworkRegistrationAllowed::<Test>::insert(net, true);
        NetworkPowRegistrationAllowed::<Test>::insert(net, false);
        assert_noop!(
            register(),
            Error::<Test>::NullConsensusPowRegistrationDisabled
        );
    });
}

#[test]
fn null_consensus_averages_rows_not_stake_or_integer_scale() {
    new_test_ext(1).execute_with(|| {
        let net = setup(4);
        // Owner has no stake; the other eligible validator has a large stake.
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &U256::from(1),
            &U256::from(101),
            net,
            1_000_000_000.into(),
        );
        SubtensorModule::set_validator_permit_for_uid(net, 1, true);
        assert_ok!(SubtensorModule::set_weights_v2(
            RuntimeOrigin::signed(U256::from(0)),
            net,
            vec![2, 3],
            vec![1, 3],
            0
        ));
        assert_ok!(SubtensorModule::set_weights_v2(
            RuntimeOrigin::signed(U256::from(1)),
            net,
            vec![2, 3],
            vec![3_000_000_000, 1_000_000_000],
            0
        ));
        let out = SubtensorModule::null_epoch(net, 1_000.into());
        assert_eq!(out[&U256::from(2)], 500.into());
        assert_eq!(out[&U256::from(3)], 500.into());
        assert!(Bonds::<Test>::iter().next().is_none());
        assert!(Weights::<Test>::iter().next().is_none());
        assert!(
            Dividends::<Test>::get(net)
                .iter()
                .all(|v| v.deconstruct() == 0)
        );
    });
}

#[test]
fn null_consensus_preserves_scores_below_u16_precision() {
    new_test_ext(1).execute_with(|| {
        let net = setup(3);
        assert_ok!(SubtensorModule::set_weights_v2(
            RuntimeOrigin::signed(U256::zero()),
            net,
            vec![1, 2],
            vec![u32::MAX, 1],
            0
        ));
        let out = SubtensorModule::null_epoch(net, (1u64 << 40).into());
        assert_eq!(out[&U256::from(2)], 256.into());
        assert_eq!(out.values().map(|a| a.to_u64()).sum::<u64>(), 1u64 << 40);
        assert_eq!(NullWeights::<Test>::get(net, 0)[1], (2, 1));
    });
}

#[test]
fn null_consensus_rejects_bad_rows_without_writes() {
    new_test_ext(1).execute_with(|| {
        let net = setup(3);
        let origin = RuntimeOrigin::signed(U256::zero());
        assert_noop!(
            SubtensorModule::set_weights_v2(origin.clone(), net, vec![1, 1], vec![1, 1], 0),
            Error::<Test>::DuplicateUids
        );
        assert_noop!(
            SubtensorModule::set_weights_v2(origin.clone(), net, vec![3], vec![1], 0),
            Error::<Test>::UidVecContainInvalidOne
        );
        assert_noop!(
            SubtensorModule::set_weights_v2(origin.clone(), net, vec![1], vec![0], 0),
            Error::<Test>::NullConsensusWeightsAllZero
        );
        assert_noop!(
            SubtensorModule::set_weights_v2(origin, net, vec![1], vec![], 0),
            Error::<Test>::WeightVecNotEqualSize
        );
        assert_noop!(
            SubtensorModule::set_weights_v2(
                RuntimeOrigin::signed(U256::from(1)),
                net,
                vec![1],
                vec![1],
                0
            ),
            Error::<Test>::NeuronNoValidatorPermit
        );
    });
}

#[test]
fn null_consensus_masks_stale_scores_and_reused_uids() {
    new_test_ext(1).execute_with(|| {
        let net = setup(3);
        assert_ok!(SubtensorModule::set_weights_v2(
            RuntimeOrigin::signed(U256::zero()),
            net,
            vec![1, 2],
            vec![1, 1],
            0
        ));
        BlockAtRegistration::<Test>::insert(net, 1, 2);
        let out = SubtensorModule::null_epoch(net, 1_000.into());
        assert!(!out.contains_key(&U256::from(1)));
        assert_eq!(out[&U256::from(2)], 1_000.into());
        System::set_block_number(100_000_000);
        assert!(SubtensorModule::null_epoch(net, 1_000.into()).is_empty());
        assert!(NullWeights::<Test>::iter().next().is_none());
    });
}

#[test]
fn null_consensus_pow_validates_seal_owner_age_and_limits() {
    new_test_ext(1).execute_with(|| {
        let net = setup(1);
        SubtensorModule::set_difficulty(net, 1);
        let hot = U256::from(8);
        let cold = U256::from(9);
        let (nonce, work) = SubtensorModule::create_work_for_block_number(net, 1, 0, &hot);
        assert_noop!(
            SubtensorModule::register(
                RuntimeOrigin::signed(cold),
                net,
                1,
                nonce,
                vec![0],
                hot,
                cold
            ),
            Error::<Test>::PowInvalidSealLength
        );
        assert_noop!(
            SubtensorModule::register(
                RuntimeOrigin::signed(hot),
                net,
                1,
                nonce,
                work.clone(),
                hot,
                cold
            ),
            Error::<Test>::PowSignerColdkeyMismatch
        );
        assert_noop!(
            SubtensorModule::register(
                RuntimeOrigin::signed(cold),
                net,
                2,
                nonce,
                work.clone(),
                hot,
                cold
            ),
            Error::<Test>::InvalidWorkBlock
        );
        assert_ok!(SubtensorModule::register(
            RuntimeOrigin::signed(cold),
            net,
            1,
            nonce,
            work.clone(),
            hot,
            cold
        ));
        assert_eq!(Uids::<Test>::get(net, hot), Some(1));
        assert_eq!(Owner::<Test>::get(hot), cold);
        assert_noop!(
            SubtensorModule::register(
                RuntimeOrigin::signed(cold),
                net,
                1,
                nonce,
                work.clone(),
                hot,
                cold
            ),
            Error::<Test>::HotKeyAlreadyRegisteredInSubNet
        );
        assert_noop!(
            SubtensorModule::register(
                RuntimeOrigin::signed(cold),
                net,
                1,
                nonce,
                work,
                U256::from(10),
                cold
            ),
            Error::<Test>::TooManyRegistrationsThisBlock
        );
        assert_eq!(Burn::<Test>::get(net), TaoBalance::ZERO);
    });
}

#[test]
fn null_consensus_full_32768_uid_vector_pays_every_scored_miner() {
    new_test_ext(1).execute_with(|| {
        let net = setup(1);
        // Seed a large metagraph directly: registration itself is tested above.
        let n: u16 = 32768;
        for uid in 1..n {
            Keys::<Test>::insert(net, uid, U256::from(uid));
            Uids::<Test>::insert(net, U256::from(uid), uid);
            BlockAtRegistration::<Test>::insert(net, uid, 1);
        }
        SubnetworkN::<Test>::insert(net, n);
        assert_ok!(SubtensorModule::set_weights_v2(
            RuntimeOrigin::signed(U256::zero()),
            net,
            (1..n).collect(),
            vec![u32::MAX; usize::from(n) - 1],
            0
        ));
        let out = SubtensorModule::null_epoch(net, 32_767_000.into());
        assert_eq!(out.len(), 32767);
        assert!(out.values().all(|&v| v == AlphaBalance::from(1000)));
        assert!(Bonds::<Test>::iter().next().is_none());
    });
}

#[test]
fn null_consensus_distributes_all_emission_to_miners_and_preserves_owner_cut() {
    new_test_ext(1).execute_with(|| {
        let net = setup(2);
        Owner::<Test>::insert(U256::from(1), U256::from(101));
        SubnetAlphaOut::<Test>::insert(net, AlphaBalance::from(650));
        assert_ok!(SubtensorModule::set_weights_v2(
            RuntimeOrigin::signed(U256::zero()),
            net,
            vec![1],
            vec![1],
            0
        ));
        SubtensorModule::distribute_emission(net, 100.into(), 200.into(), 300.into(), 50.into());
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_on_subnet(&U256::from(1), net),
            600.into()
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_on_subnet(&U256::zero(), net),
            50.into()
        );
        assert_eq!(SubnetAlphaOut::<Test>::get(net), 650.into());
        assert!(Bonds::<Test>::iter().next().is_none());
    });
}

#[test]
fn null_consensus_empty_epoch_recycles_unallocated_alpha() {
    new_test_ext(1).execute_with(|| {
        let net = setup(2);
        SubnetAlphaOut::<Test>::insert(net, AlphaBalance::from(650));
        SubtensorModule::distribute_emission(net, 100.into(), 200.into(), 300.into(), 50.into());
        assert_eq!(SubnetAlphaOut::<Test>::get(net), 50.into());
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_on_subnet(&U256::zero(), net),
            50.into()
        );
    });
}

#[test]
fn null_consensus_used_work_cannot_follow_a_hotkey_to_another_subnet() {
    new_test_ext(1).execute_with(|| {
        let net = setup(1);
        let other = NetUid::from(2);
        SubtensorModule::init_new_network(other, 360);
        assert_ok!(SubtensorModule::enable_null_consensus(
            RuntimeOrigin::root(),
            other
        ));
        SubtensorModule::set_difficulty(net, 1);
        SubtensorModule::set_difficulty(other, 1);
        let hotkey = U256::from(8);
        let coldkey = U256::from(9);
        let (nonce, work) = SubtensorModule::create_work_for_block_number(net, 1, 0, &hotkey);
        assert_ok!(SubtensorModule::register(
            RuntimeOrigin::signed(coldkey),
            net,
            1,
            nonce,
            work.clone(),
            hotkey,
            coldkey
        ));
        assert_noop!(
            SubtensorModule::register(
                RuntimeOrigin::signed(coldkey),
                other,
                1,
                nonce,
                work,
                hotkey,
                coldkey
            ),
            Error::<Test>::PowWorkAlreadyUsed
        );
        SubnetworkN::<Test>::insert(other, 32768);
        assert_noop!(
            SubtensorModule::register(
                RuntimeOrigin::signed(coldkey),
                other,
                1,
                0,
                vec![0; 32],
                U256::from(10),
                coldkey
            ),
            Error::<Test>::NullConsensusCapacityReached
        );
    });
}
