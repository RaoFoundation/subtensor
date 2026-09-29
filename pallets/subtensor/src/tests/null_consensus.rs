#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
use super::mock::*;
use crate::*;
use frame_support::{assert_noop, assert_ok};
use sp_core::U256;

fn setup(n: u16) -> NetUid {
    let net = NetUid::from(1);
    System::set_block_number(1);
    SubtensorModule::init_new_network(net, 360);
    SubnetOwner::<Test>::insert(net, U256::from(100));
    SubnetOwnerHotkey::<Test>::insert(net, U256::zero());
    for uid in 0..n {
        Owner::<Test>::insert(U256::from(uid), U256::from(100 + uid));
        SubtensorModule::append_neuron(net, &U256::from(uid), 1);
    }
    assert_ok!(SubtensorModule::do_set_null_consensus(net, true));
    NetworkPowRegistrationAllowed::<Test>::insert(net, true);
    MaxRegistrationsPerBlock::<Test>::insert(net, 10);
    SubtensorModule::set_difficulty(net, 1);
    System::set_block_number(2);
    net
}
fn register(net: NetUid, hot: u64, cold: u64) -> DispatchResult {
    let hot = U256::from(hot);
    let cold = U256::from(cold);
    let seal = SubtensorModule::create_null_seal_hash(net, 1, 0, &hot, &cold);
    SubtensorModule::do_null_pow_register(
        RuntimeOrigin::signed(cold),
        net,
        1,
        0,
        seal.as_bytes().to_vec(),
        hot,
        cold,
    )
}
fn claim(net: NetUid, hot: u64, cold: u64) -> DispatchResult {
    SubtensorModule::do_claim_null_rewards(
        RuntimeOrigin::signed(U256::from(cold)),
        net,
        U256::from(hot),
        U256::zero(),
    )
}
fn pending(net: NetUid, hot: u64) -> u64 {
    let (_, checkpoint) = NullMiners::<Test>::get(net, U256::from(hot)).unwrap();
    ((NullRewardIndex::<Test>::get(net) - checkpoint) >> 64).low_u64()
}
#[test]
fn null_equal_rewards_do_not_need_epochs_or_weights() {
    new_test_ext(1).execute_with(|| {
        let net = setup(3);
        SubnetAlphaOut::<Test>::insert(net, AlphaBalance::from(900u64));
        SubtensorModule::accrue_null_rewards(net, 900u64.into());
        assert_eq!(
            (pending(net, 0), pending(net, 1), pending(net, 2)),
            (300, 300, 300)
        );
        let before = SubnetworkN::<Test>::get(net);
        assert_ok!(claim(net, 1, 101));
        assert_eq!(SubnetAlphaOut::<Test>::get(net), AlphaBalance::from(900u64));
        assert_eq!(
            TotalHotkeyAlpha::<Test>::get(U256::zero(), net),
            AlphaBalance::from(300u64)
        );
        assert_eq!(
            NullUnclaimedAlpha::<Test>::get(net),
            AlphaBalance::from(600u64)
        );
        assert_eq!(SubnetworkN::<Test>::get(net), before);
        assert_noop!(claim(net, 1, 101), Error::<Test>::NullRewardsNotAvailable);
        assert_noop!(claim(net, 2, 101), Error::<Test>::NonAssociatedColdKey);
    });
}
#[test]
fn null_pow_registration_has_no_population_sized_writes_or_historical_rewards() {
    new_test_ext(1).execute_with(|| {
        let net = setup(1);
        SubtensorModule::accrue_null_rewards(net, 100u64.into());
        let legacy = (
            SubnetworkN::<Test>::get(net),
            ValidatorPermit::<Test>::get(net),
            Emission::<Test>::get(net),
        );
        assert_ok!(register(net, 900, 901));
        assert_eq!(pending(net, 900), 0);
        assert_eq!(pending(net, 0), 100);
        assert_eq!(NullMinerCount::<Test>::get(net), 2);
        assert_eq!(
            legacy,
            (
                SubnetworkN::<Test>::get(net),
                ValidatorPermit::<Test>::get(net),
                Emission::<Test>::get(net)
            )
        );
        assert!(!Owner::<Test>::contains_key(U256::from(900)));
        assert!(OwnedHotkeys::<Test>::get(U256::from(901)).is_empty());
        assert!(StakingHotkeys::<Test>::get(U256::from(901)).is_empty());
        assert!(!MinerCollateral::<Test>::contains_key((
            net,
            U256::from(900),
            U256::from(901)
        )));
        SubtensorModule::accrue_null_rewards(net, 100u64.into());
        assert_eq!((pending(net, 0), pending(net, 900)), (150, 50));
    });
}
#[test]
fn null_u64_population_can_switch_modes_without_changing_yuma_capacity() {
    new_test_ext(1).execute_with(|| {
        let net = setup(1);
        let capacity = MaxAllowedUids::<Test>::get(net);
        NullMinerCount::<Test>::insert(net, 1u64 << 40);
        assert_ok!(register(net, 900, 901));
        assert_eq!(
            NullMinerKeys::<Test>::get(net, 1u64 << 40),
            Some(U256::from(900))
        );
        assert_ok!(SubtensorModule::do_set_null_consensus(net, false));
        assert_eq!(SubnetworkN::<Test>::get(net), 1);
        assert_eq!(MaxAllowedUids::<Test>::get(net), capacity);
        assert_ok!(SubtensorModule::do_set_null_consensus(net, true));
        assert_eq!(NullMinerCount::<Test>::get(net), (1u64 << 40) + 1);
        SubtensorModule::accrue_null_rewards(net, ((1u64 << 40) + 1).into());
        assert_eq!(pending(net, 900), 1);
    });
}
#[test]
fn null_fractional_rewards_and_claims_conserve_the_budget() {
    new_test_ext(1).execute_with(|| {
        let net = setup(3);
        for _ in 0..100 {
            SubtensorModule::accrue_null_rewards(net, 1u64.into());
        }
        for uid in 0..3 {
            assert_eq!(pending(net, uid), 33);
            assert_ok!(claim(net, uid, 100 + uid));
        }
        assert_eq!(
            NullUnclaimedAlpha::<Test>::get(net),
            AlphaBalance::from(1u64)
        );
        for _ in 0..2 {
            SubtensorModule::accrue_null_rewards(net, 1u64.into());
        }
        for uid in 0..3 {
            assert_eq!(pending(net, uid), 1);
            assert_ok!(claim(net, uid, 100 + uid));
        }
        assert_eq!(NullUnclaimedAlpha::<Test>::get(net), AlphaBalance::ZERO);
        assert_eq!(
            TotalHotkeyAlpha::<Test>::get(U256::zero(), net),
            AlphaBalance::from(102u64)
        );
    });
}
#[test]
fn null_claims_survive_mode_changes_and_do_not_consume_paused_yuma_emission() {
    new_test_ext(1).execute_with(|| {
        let net = setup(1);
        SubtensorModule::accrue_null_rewards(net, 100u64.into());
        assert_ok!(SubtensorModule::do_set_null_consensus(net, false));
        PendingServerEmission::<Test>::insert(net, AlphaBalance::from(50u64));
        assert_ok!(claim(net, 0, 100));
        assert_ok!(SubtensorModule::do_set_null_consensus(net, true));
        assert_eq!(pending(net, 0), 0);
        assert_eq!(PendingServerEmission::<Test>::get(net), AlphaBalance::ZERO);
        assert_ok!(SubtensorModule::do_set_null_consensus(net, false));
        assert_eq!(
            PendingServerEmission::<Test>::get(net),
            AlphaBalance::from(50u64)
        );
    });
}
#[test]
fn null_pow_is_bound_to_recipient_subnet_generation_and_mode() {
    new_test_ext(1).execute_with(|| {
        let net = setup(1);
        let hot = U256::from(900);
        let cold = U256::from(901);
        let seal = SubtensorModule::create_null_seal_hash(net, 1, 0, &hot, &cold)
            .as_bytes()
            .to_vec();
        assert_noop!(
            SubtensorModule::do_null_pow_register(
                RuntimeOrigin::signed(U256::from(902)),
                net,
                1,
                0,
                seal.clone(),
                hot,
                U256::from(902)
            ),
            Error::<Test>::InvalidSeal
        );
        RegisteredSubnetCounter::<Test>::mutate(net, |n| *n += 1);
        assert_noop!(
            SubtensorModule::do_null_pow_register(
                RuntimeOrigin::signed(cold),
                net,
                1,
                0,
                seal.clone(),
                hot,
                cold
            ),
            Error::<Test>::InvalidSeal
        );
        assert_ok!(SubtensorModule::do_set_null_consensus(net, false));
        assert_noop!(
            SubtensorModule::do_null_pow_register(
                RuntimeOrigin::signed(cold),
                net,
                1,
                0,
                seal,
                hot,
                cold
            ),
            Error::<Test>::NullConsensusNotEnabled
        );
    });
}
#[test]
fn null_registration_errors_and_reward_free_scores_are_explicit() {
    new_test_ext(1).execute_with(|| {
        let net = setup(1);
        NetworkPowRegistrationAllowed::<Test>::insert(net, false);
        assert_noop!(
            register(net, 900, 901),
            Error::<Test>::NullConsensusPowRegistrationDisabled
        );
        NetworkPowRegistrationAllowed::<Test>::insert(net, true);
        NetworkRegistrationAllowed::<Test>::insert(net, false);
        assert_noop!(
            register(net, 900, 901),
            Error::<Test>::NullConsensusRegistrationDisabled
        );
        assert_noop!(
            SubtensorModule::set_null_weights(
                RuntimeOrigin::signed(U256::zero()),
                net,
                vec![0],
                vec![u32::MAX],
                0
            ),
            Error::<Test>::NullConsensusHasNoWeights
        );
        assert_noop!(
            SubtensorModule::do_enable_voting_power_tracking(net),
            Error::<Test>::NullConsensusHasNoValidators
        );
    });
}
#[test]
fn null_never_enters_the_epoch_scheduler_even_when_triggered() {
    new_test_ext(1).execute_with(|| {
        let net = setup(1);
        PendingEpochAt::<Test>::insert(net, 2);
        assert!(SubtensorModule::drain_pending(&[net], 2).is_empty());
        assert_eq!(LastEpochBlock::<Test>::get(net), 0);
        assert!(NullWeights::<Test>::iter_prefix(net).next().is_none());
    });
}

#[test]
fn null_low_root_price_cannot_recycle_the_miners_budget() {
    use alloc::collections::BTreeMap;
    use substrate_fixed::types::U96F32;
    new_test_ext(1).execute_with(|| {
        let net = add_dynamic_network(&U256::zero(), &U256::from(100));
        SubnetOwnerCut::<Test>::set(u16::MAX / 10);
        assert_ok!(SubtensorModule::do_set_null_consensus(net, true));
        setup_reserves(
            net,
            1_000_000_000_000_000u64.into(),
            1_000_000_000_000_000u64.into(),
        );
        assert_ok!(Swap::maybe_initialize_palswap(net, None));
        let owner_before = TotalHotkeyAlpha::<Test>::get(U256::zero(), net);
        let credit = SubtensorModule::mint_tao(12345678u64.into());
        SubtensorModule::emit_to_subnets(
            &[net],
            &BTreeMap::from([(net, U96F32::from_num(12345678u64))]),
            credit,
            false,
        );
        let owner = TotalHotkeyAlpha::<Test>::get(U256::zero(), net).saturating_sub(owner_before);
        assert!(owner > AlphaBalance::ZERO);
        assert_eq!(
            NullUnclaimedAlpha::<Test>::get(net).saturating_add(owner),
            SubnetAlphaOutEmission::<Test>::get(net)
        );
        assert_eq!(PendingRootAlphaDivs::<Test>::get(net), AlphaBalance::ZERO);
        assert_eq!(
            PendingValidatorEmission::<Test>::get(net),
            AlphaBalance::ZERO
        );
    });
}

#[test]
fn null_dissolution_settles_rewards_in_bounded_steps_before_stake_conversion() {
    use crate::subnets::dissolution::{DissolveCleanupPhase, DissolveCleanupStatus};
    use crate::weights::WeightInfo;
    use frame_support::weights::WeightMeter;
    new_test_ext(1).execute_with(|| {
        let net = setup(3);
        SubtensorModule::accrue_null_rewards(net, 300u64.into());
        let mut status = DissolveCleanupStatus::new(net);
        let bound = <Test as Config>::WeightInfo::claim_null_rewards();
        for remaining in [2, 1, 0] {
            let mut meter = WeightMeter::with_limit(bound);
            let (done, _) =
                SubtensorModule::clean_up_data_for_one_dissolved_network(&mut meter, &mut status);
            assert!(!done);
            assert_eq!(
                status.phase,
                DissolveCleanupPhase::SubnetBasketHoldingsToRoot
            );
            assert_eq!(NullMiners::<Test>::iter_prefix(net).count(), remaining);
        }
        assert_eq!(NullUnclaimedAlpha::<Test>::get(net), AlphaBalance::ZERO);
        assert_eq!(
            TotalHotkeyAlpha::<Test>::get(U256::zero(), net),
            AlphaBalance::from(300u64)
        );
    });
}

#[test]
fn null_claims_follow_coldkey_swaps_without_confusing_reused_addresses() {
    new_test_ext(1).execute_with(|| {
        let net = setup(1);
        assert_ok!(register(net, 900, 901));
        SubtensorModule::accrue_null_rewards(net, 100u64.into());
        SubtensorModule::record_coldkey_swap_lineage(&U256::from(901), &U256::from(902));
        assert_noop!(claim(net, 900, 901), Error::<Test>::NonAssociatedColdKey);
        assert_ok!(claim(net, 900, 902));
        // Reuse the old coldkey for a different miner; it owns the new generation.
        assert_ok!(register(net, 903, 901));
        SubtensorModule::accrue_null_rewards(net, 300u64.into());
        assert_noop!(claim(net, 903, 902), Error::<Test>::NonAssociatedColdKey);
        assert_ok!(claim(net, 903, 901));
        assert_ok!(claim(net, 900, 902));
    });
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
fn null_long_coldkey_history_resolves_in_bounded_claim_steps() {
    new_test_ext(1).execute_with(|| {
        let net = setup(1);
        assert_ok!(register(net, 900, 901));
        SubtensorModule::accrue_null_rewards(net, 100u64.into());
        for cold in 901..971 {
            SubtensorModule::record_coldkey_swap_lineage(&U256::from(cold), &U256::from(cold + 1));
        }
        assert_ok!(claim(net, 900, 971));
        assert_eq!(pending(net, 900), 50);
        assert_eq!(
            NullMiners::<Test>::get(net, U256::from(900)).unwrap().0,
            U256::from(965)
        );
        assert_ok!(claim(net, 900, 971));
        assert_eq!(pending(net, 900), 0);
        assert_eq!(
            TotalHotkeyAlpha::<Test>::get(U256::zero(), net),
            AlphaBalance::from(50u64)
        );
    });
}

#[test]
fn null_clears_validator_power_and_completes_a_scheduled_disable_without_epochs() {
    new_test_ext(1).execute_with(|| {
        let net = setup(2);
        assert_ok!(SubtensorModule::do_set_null_consensus(net, false));
        assert_ok!(SubtensorModule::do_enable_voting_power_tracking(net));
        VotingPower::<Test>::insert(net, U256::zero(), 100);
        TotalVotingPower::<Test>::insert(net, 100);
        assert_ok!(SubtensorModule::do_disable_voting_power_tracking(net));
        let deadline = VotingPowerDisableAtBlock::<Test>::get(net);
        assert_ok!(SubtensorModule::do_set_null_consensus(net, true));
        assert_eq!(TotalVotingPower::<Test>::get(net), 0);
        assert!(VotingPower::<Test>::iter_prefix(net).next().is_none());
        assert!(VotingPowerTrackingEnabled::<Test>::get(net));
        System::set_block_number(deadline);
        assert!(SubtensorModule::drain_pending(&[net], deadline).is_empty());
        assert!(!VotingPowerTrackingEnabled::<Test>::get(net));
        assert_eq!(VotingPowerDisableAtBlock::<Test>::get(net), 0);
    });
}
