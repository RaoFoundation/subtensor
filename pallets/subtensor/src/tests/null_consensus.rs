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
    assert_ok!(SubtensorModule::do_set_null_consensus(net, true));
    SubtensorModule::set_max_allowed_uids(net, 32768);
    SubtensorModule::set_max_allowed_validators(net, 1);
    MaxRegistrationsPerBlock::<Test>::insert(net, 1);
    NetworkPowRegistrationAllowed::<Test>::insert(net, true);
    for uid in 1..n {
        SubtensorModule::append_neuron(net, &U256::from(uid), 1);
    }
    SubtensorModule::set_weights_set_rate_limit(net, 0);
    SubtensorModule::set_stake_threshold(0);
    System::set_block_number(2);
    net
}

#[test]
fn null_consensus_pow_request_rejects_a_mode_change_without_payment() {
    new_test_ext(1).execute_with(|| {
        let net = setup(1);
        let cold = U256::from(900);
        let hot = U256::from(901);
        add_balance_to_coldkey_account(&cold, 10_000_000_000u64.into());
        SubtensorModule::set_difficulty(net, 1);
        let (nonce, work) = SubtensorModule::create_work_for_block_number(net, 1, 0, &hot);
        SubtensorModule::set_max_allowed_uids(net, 16);
        assert_ok!(SubtensorModule::do_set_null_consensus(net, false));
        CollateralLockShare::<Test>::insert(net, 32768);
        for signed_work in [work, vec![1]] {
            // Even malformed nonempty work cannot be interpreted as consent to pay.
            assert_noop!(
                SubtensorModule::register(
                    RuntimeOrigin::signed(cold),
                    net,
                    1,
                    nonce,
                    signed_work,
                    hot,
                    cold
                )
                .map_err(|error| error.error),
                Error::<Test>::NullConsensusNotEnabled
            );
        }
    });
}

#[test]
fn null_consensus_conviction_successor_registers_with_admission_paused_or_full() {
    use crate::staking::lock::{LockState, ONE_YEAR};
    use substrate_fixed::types::U64F64;

    for full in [false, true] {
        new_test_ext(1).execute_with(|| {
            let net = setup(2);
            SubtensorModule::set_max_allowed_uids(net, if full { 2 } else { 3 });
            NetworkRegistrationAllowed::<Test>::insert(net, false);
            NetworkPowRegistrationAllowed::<Test>::insert(net, false);
            let cold = U256::from(900);
            let successor = U256::from(901);
            assert_ok!(SubtensorModule::create_account_if_non_existent(
                &cold, &successor
            ));
            let now = ONE_YEAR + 1;
            System::set_block_number(now);
            NetworkRegisteredAt::<Test>::insert(net, 1);
            SubnetAlphaOut::<Test>::insert(net, AlphaBalance::from(10_000u64));
            // Admission remains protocol-only and still requires qualifying conviction.
            for conviction in [100u64, 2000] {
                let lock = LockState {
                    locked_mass: conviction.into(),
                    conviction: U64F64::from_num(conviction),
                    last_update: now,
                };
                Lock::<Test>::insert((cold, net, successor), lock.clone());
                HotkeyLock::<Test>::insert(net, successor, lock);
                SubtensorModule::change_subnet_owner_if_needed(net);
                if conviction == 100 {
                    assert_eq!(SubnetOwner::<Test>::get(net), U256::from(100));
                    assert!(!Uids::<Test>::contains_key(net, successor));
                }
            }
            assert_eq!(SubnetOwner::<Test>::get(net), cold);
            assert_eq!(SubnetOwnerHotkey::<Test>::get(net), successor);
            let uid = if full { 0 } else { 2 };
            assert_eq!(Uids::<Test>::get(net, successor), Some(uid));
            assert_eq!(Keys::<Test>::get(net, uid), successor);
            assert!(IsNetworkMember::<Test>::get(successor, net));
            assert_eq!(SubnetworkN::<Test>::get(net), if full { 2 } else { 3 });
            assert_eq!(Keys::<Test>::get(net, 1), U256::from(1));
            if full {
                assert!(!Uids::<Test>::contains_key(net, U256::zero()));
                assert!(!IsNetworkMember::<Test>::get(U256::zero(), net));
                assert_eq!(BlockAtRegistration::<Test>::get(net, uid), now);
            }
            assert_noop!(
                SubtensorModule::register_neuron(net, &U256::from(999)),
                Error::<Test>::NullConsensusRequiresPowRegistration
            );
        });
    }
}

#[test]
fn null_consensus_epochs_maintain_voting_power_and_complete_scheduled_disable() {
    new_test_ext(1).execute_with(|| {
        let net = setup(3);
        let old = U256::from(1);
        let new = U256::from(2);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &new,
            &U256::from(902),
            net,
            200u64.into(),
        );
        VotingPowerTrackingEnabled::<Test>::insert(net, true);
        VotingPowerEmaAlpha::<Test>::insert(net, 1_000_000_000_000_000_000u64);
        VotingPower::<Test>::insert(net, old, 300);
        TotalVotingPower::<Test>::insert(net, 300);
        let epoch =
            || SubtensorModule::distribute_emission(net, 0.into(), 0.into(), 0.into(), 0.into());
        epoch();
        assert_eq!(VotingPower::<Test>::get(net, old), 0);
        assert_eq!(VotingPower::<Test>::get(net, new), 200);
        assert_eq!(TotalVotingPower::<Test>::get(net), 200);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &new,
            &U256::from(902),
            net,
            100u64.into(),
        );
        epoch();
        assert_eq!(VotingPower::<Test>::get(net, new), 300);
        assert_eq!(TotalVotingPower::<Test>::get(net), 300);
        assert_ok!(SubtensorModule::do_disable_voting_power_tracking(net));
        System::set_block_number(VotingPowerDisableAtBlock::<Test>::get(net));
        epoch();
        assert!(!VotingPowerTrackingEnabled::<Test>::get(net));
        assert!(!VotingPowerDisableAtBlock::<Test>::contains_key(net));
        assert!(VotingPower::<Test>::iter_prefix(net).next().is_none());
        assert_eq!(TotalVotingPower::<Test>::get(net), 0);
    });
}

#[test]
fn null_consensus_profile_is_opt_in() {
    new_test_ext(1).execute_with(|| {
        let net = setup(1);
        assert!(NullConsensus::<Test>::get(net));
        assert_eq!(MaxAllowedUids::<Test>::get(net), 32768);
        assert!(Yuma3On::<Test>::get(net));
        assert!(CommitRevealWeightsEnabled::<Test>::get(net));
        assert!(!NullConsensus::<Test>::get(NetUid::from(2)));
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
fn null_consensus_toggle_preserves_live_yuma_state_and_hyperparameters() {
    new_test_ext(1).execute_with(|| {
        let net = NetUid::from(1);
        SubtensorModule::init_new_network(net, 360);
        assert!(Yuma3On::<Test>::get(net));
        assert!(!NullConsensus::<Test>::get(net));
        SubnetOwner::<Test>::insert(net, U256::from(100));
        SubnetOwnerHotkey::<Test>::insert(net, U256::zero());
        for uid in 0..3 {
            SubtensorModule::append_neuron(net, &U256::from(uid), 1);
        }
        FirstEmissionBlockNumber::<Test>::insert(net, 1);
        let index = NetUidStorageIndex::from(net);
        Weights::<Test>::insert(index, 0, vec![(1, u16::MAX)]);
        Bonds::<Test>::insert(index, 0, vec![(1, 123)]);
        let config = || {
            (
                MaxAllowedUids::<Test>::get(net),
                MaxAllowedValidators::<Test>::get(net),
                Tempo::<Test>::get(net),
                Burn::<Test>::get(net),
                MinBurn::<Test>::get(net),
                Difficulty::<Test>::get(net),
                CollateralLockShare::<Test>::get(net),
                CommitRevealWeightsEnabled::<Test>::get(net),
                LiquidAlphaOn::<Test>::get(net),
                WeightsSetRateLimit::<Test>::get(net),
                NetworkPowRegistrationAllowed::<Test>::get(net),
            )
        };
        let original = config();
        let last_update = LastUpdate::<Test>::get(index);
        System::set_block_number(3);
        for _ in 0..2 {
            assert_ok!(SubtensorModule::do_set_null_consensus(net, true));
            assert_eq!(config(), original);
            assert!(Yuma3On::<Test>::get(net));
            assert_ok!(SubtensorModule::do_set_null_consensus(net, true)); // idempotent
            assert_ok!(SubtensorModule::do_set_null_consensus(net, false));
            assert!(!NullConsensus::<Test>::get(net));
            assert_eq!(config(), original);
        }
        assert_ok!(SubtensorModule::do_set_null_consensus(net, true));
        System::set_block_number(4);
        assert_ok!(SubtensorModule::set_weights_v2(
            RuntimeOrigin::signed(U256::zero()),
            net,
            vec![2],
            vec![u32::MAX],
            0
        ));
        assert_eq!(NullLastUpdate::<Test>::get(net, 0), 4);
        assert_eq!(LastUpdate::<Test>::get(index), last_update);
        assert_eq!(Weights::<Test>::get(index, 0), vec![(1, u16::MAX)]);
        assert_eq!(Bonds::<Test>::get(index, 0), vec![(1, 123)]);
        assert_ok!(SubtensorModule::do_set_null_consensus(net, false));
        assert_noop!(
            SubtensorModule::set_weights_v2(
                RuntimeOrigin::signed(U256::zero()),
                net,
                vec![2],
                vec![1],
                0
            ),
            Error::<Test>::NullConsensusNotEnabled
        );
        assert_eq!(LastUpdate::<Test>::get(index), last_update);
        assert_eq!(Weights::<Test>::get(index, 0), vec![(1, u16::MAX)]);
        assert_eq!(Bonds::<Test>::get(index, 0), vec![(1, 123)]);
    });
}

#[test]
fn null_consensus_toggle_rejects_unsupported_capacity_without_mutation() {
    new_test_ext(1).execute_with(|| {
        let net = setup(3);
        assert_noop!(
            SubtensorModule::do_set_null_consensus(net, false),
            Error::<Test>::NullConsensusYumaCapacityExceeded
        );
        MaxAllowedUids::<Test>::insert(net, DefaultMaxAllowedUids::<Test>::get());
        assert_ok!(SubtensorModule::do_set_null_consensus(net, false));
        MechanismCountCurrent::<Test>::insert(net, MechId::from(2));
        assert_noop!(
            SubtensorModule::do_set_null_consensus(net, true),
            Error::<Test>::NullConsensusRequiresSingleMechanism
        );
    });
}

#[test]
fn null_consensus_caps_cached_rows_across_mode_changes() {
    new_test_ext(1).execute_with(|| {
        let net = setup(66);
        for uid in 1..=65 {
            NullWeights::<Test>::insert(net, uid, vec![(0, 1u32)]);
            NullLastUpdate::<Test>::insert(net, uid, 2);
        }
        assert_noop!(
            SubtensorModule::set_weights_v2(
                RuntimeOrigin::signed(U256::zero()),
                net,
                vec![1],
                vec![1],
                0
            ),
            Error::<Test>::NullConsensusValidatorLimitExceeded
        );
        // The first null epoch clears rows from former Yuma permit holders.
        SubtensorModule::null_epoch(net, 1000.into());
        assert_ok!(SubtensorModule::set_weights_v2(
            RuntimeOrigin::signed(U256::zero()),
            net,
            vec![1],
            vec![1],
            0
        ));
        assert_eq!(NullWeights::<Test>::iter_key_prefix(net).count(), 1);
    });
}

#[test]
fn null_consensus_trimming_discards_scores_before_uid_reuse() {
    new_test_ext(1).execute_with(|| {
        let net = setup(4);
        MinAllowedUids::<Test>::insert(net, 1);
        ImmunityPeriod::<Test>::insert(net, 0);
        System::set_block_number(10000);
        assert_ok!(SubtensorModule::set_weights_v2(
            RuntimeOrigin::signed(U256::zero()),
            net,
            vec![3],
            vec![1],
            0
        ));
        SubtensorModule::set_max_allowed_uids(net, 4);
        assert_ok!(SubtensorModule::do_set_null_consensus(net, false));
        assert_ok!(SubtensorModule::trim_to_max_allowed_uids(net, 3));
        assert_ok!(SubtensorModule::do_set_null_consensus(net, true));
        assert!(SubtensorModule::null_epoch(net, 1000.into()).is_empty());
        assert!(NullWeights::<Test>::iter_prefix(net).next().is_none());
        System::set_block_number(10001);
        assert_ok!(SubtensorModule::set_weights_v2(
            RuntimeOrigin::signed(U256::zero()),
            net,
            vec![1],
            vec![1],
            0
        ));
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
        assert_eq!(Burn::<Test>::get(net), DefaultNeuronBurnCost::<Test>::get());
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
        assert_ok!(SubtensorModule::do_set_null_consensus(other, true));
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
