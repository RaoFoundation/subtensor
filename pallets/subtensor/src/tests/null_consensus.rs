#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
use super::mock::*;
use crate::*;
use frame_support::{assert_noop, assert_ok};
use sp_core::U256;
use subtensor_runtime_common::MechId;

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
fn null_claims_respect_staking_hotkey_capacity_without_losing_rewards() {
    for paused in [false, true] {
        new_test_ext(1).execute_with(|| {
            let net = setup(2);
            let cold = U256::from(101);
            let miner = U256::from(1);
            let target = U256::zero();
            for index in 0..MAX_STAKING_HOTKEYS - 1 {
                let hotkey = U256::from(1000 + index);
                SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
                    &hotkey,
                    &cold,
                    net,
                    100u64.into(),
                );
            }
            if paused {
                assert_ok!(SubtensorModule::do_set_null_consensus(net, false));
            }
            SubtensorModule::accrue_null_rewards(net, 200u64.into());
            assert_ok!(claim(net, 1, 101));
            assert_eq!(
                StakingHotkeys::<Test>::get(cold).len(),
                MAX_STAKING_HOTKEYS as usize
            );
            SubtensorModule::accrue_null_rewards(net, 200u64.into());
            // The miner is a valid staking account but not yet a destination in this list.
            assert_noop!(
                SubtensorModule::do_claim_null_rewards(
                    RuntimeOrigin::signed(cold),
                    net,
                    miner,
                    miner,
                ),
                Error::<Test>::TooManyStakingHotkeys
            );
            assert_eq!(pending(net, 1), 100);
            // Existing destinations remain usable at capacity, including after refusal.
            assert_ok!(claim(net, 1, 101));
            assert_eq!(pending(net, 1), 0);
            assert_eq!(
                StakingHotkeys::<Test>::get(cold).len(),
                MAX_STAKING_HOTKEYS as usize
            );
            assert_eq!(
                SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&target, &cold, net),
                AlphaBalance::from(200u64),
            );
        });
    }
}

#[test]
fn null_claims_settle_existing_collateral_before_staking_the_remainder() {
    for floor in [0u64, 120, 200] {
        for paused in [false, true] {
            new_test_ext(1).execute_with(|| {
                let net = setup(2);
                let hot = U256::from(1);
                let cold = U256::from(101);
                let target = U256::zero();
                assert_ok!(SubtensorModule::do_set_null_consensus(net, false));
                SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
                    &hot,
                    &cold,
                    net,
                    100u64.into(),
                );
                MinerCollateral::<Test>::insert(
                    (net, hot, cold),
                    MinerCollateralState {
                        locked: 100u64.into(),
                        drain_ratio: substrate_fixed::types::U64F64::from_num(2),
                        min_locked: floor.into(),
                        earned: AlphaBalance::ZERO,
                    },
                );
                ColdkeyMinerCollateral::<Test>::insert(net, cold, AlphaBalance::from(100u64));
                let index: frame_support::BoundedVec<
                    _,
                    frame_support::traits::ConstU32<MAX_COLDKEY_COLLATERAL_HOTKEYS>,
                > = vec![hot].try_into().unwrap();
                ColdkeyCollateralHotkeys::<Test>::insert(net, cold, index);
                // Settlement must retain the registration's ratio snapshot.
                CollateralDrainRatio::<Test>::insert(
                    net,
                    substrate_fixed::types::U64F64::from_num(9),
                );
                assert_ok!(SubtensorModule::do_set_null_consensus(net, true));
                SubtensorModule::accrue_null_rewards(net, 80u64.into());
                if paused {
                    assert_ok!(SubtensorModule::do_set_null_consensus(net, false));
                }
                assert_ok!(claim(net, 1, 101));
                let captured = floor.saturating_sub(100).min(40);
                let locked = if floor == 0 { 20 } else { 100 + captured };
                let state = MinerCollateral::<Test>::get((net, hot, cold)).unwrap();
                assert_eq!(state.locked, AlphaBalance::from(locked));
                assert_eq!(state.earned, AlphaBalance::from(40u64));
                assert_eq!(
                    state.drain_ratio,
                    substrate_fixed::types::U64F64::from_num(2)
                );
                assert_eq!(ColdkeyMinerCollateral::<Test>::get(net, cold), state.locked);
                assert_eq!(
                    SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hot, &cold, net),
                    AlphaBalance::from(100 + captured)
                );
                assert_eq!(
                    SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                        &target, &cold, net
                    ),
                    AlphaBalance::from(40 - captured)
                );
                assert_eq!(pending(net, 1), 0);
                assert_eq!(
                    NullUnclaimedAlpha::<Test>::get(net),
                    AlphaBalance::from(40u64)
                );
                assert_noop!(claim(net, 1, 101), Error::<Test>::NullRewardsNotAvailable);
                if floor == 0 {
                    // The final drain removes the lock and both aggregate/index
                    // entries; replaying a claim cannot drain it twice.
                    SubtensorModule::accrue_null_rewards(net, 80u64.into());
                    assert_ok!(claim(net, 1, 101));
                    assert!(!MinerCollateral::<Test>::contains_key((net, hot, cold)));
                    assert!(!ColdkeyMinerCollateral::<Test>::contains_key(net, cold));
                    assert!(ColdkeyCollateralHotkeys::<Test>::get(net, cold).is_empty());
                    assert_eq!(
                        SubtensorModule::available_to_unstake_from_hotkey(&cold, &hot, net),
                        AlphaBalance::from(100u64)
                    );
                }
            });
        }
    }
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
fn null_pow_cannot_be_replayed_after_hotkey_rotation() {
    for all_subnets in [false, true] {
        for staking_owner in [false, true] {
            new_test_ext(1).execute_with(|| {
                let net = setup(1);
                let old = U256::from(900);
                let new = U256::from(902);
                let cold = U256::from(901);
                assert_ok!(register(net, 900, 901));
                let work = SubtensorModule::create_null_seal_hash(net, 1, 0, &old, &cold)
                    .as_bytes()
                    .to_vec();
                assert_eq!(UsedWork::<Test>::get(&work), 2);
                if staking_owner {
                    assert_ok!(SubtensorModule::create_account_if_non_existent(&cold, &old));
                }
                add_balance_to_coldkey_account(&cold, 1_000_000_000_000u64.into());
                assert_ok!(SubtensorModule::do_swap_hotkey(
                    RuntimeOrigin::signed(cold),
                    &old,
                    &new,
                    if all_subnets { None } else { Some(net) },
                    false,
                ));
                // Both blocks remain inside the seal's validity window.
                for block in [2, 3] {
                    System::set_block_number(block);
                    assert_noop!(register(net, 900, 901), Error::<Test>::PowWorkAlreadyUsed);
                    assert_eq!(NullMinerCount::<Test>::get(net), 2);
                    assert!(!NullMiners::<Test>::contains_key(net, old));
                    assert!(NullMiners::<Test>::contains_key(net, new));
                }
                // A fresh nonce is new work and may create one more share.
                let fresh = SubtensorModule::create_null_seal_hash(net, 1, 1, &old, &cold)
                    .as_bytes()
                    .to_vec();
                assert_ok!(SubtensorModule::do_null_pow_register(
                    RuntimeOrigin::signed(cold),
                    net,
                    1,
                    1,
                    fresh.clone(),
                    old,
                    cold,
                ));
                assert_eq!(NullMinerCount::<Test>::get(net), 3);
                assert_eq!(UsedWork::<Test>::get(&fresh), 3);
                assert_eq!(UsedWork::<Test>::get(&work), 2);
            });
        }
    }
}

#[test]
fn failed_null_registration_does_not_consume_work() {
    new_test_ext(1).execute_with(|| {
        let net = setup(1);
        let count = NullMinerCount::<Test>::get(net);
        NullMinerCount::<Test>::insert(net, u64::MAX);
        assert_noop!(
            register(net, 900, 901),
            Error::<Test>::NullConsensusCapacityReached
        );
        NullMinerCount::<Test>::insert(net, count);
        assert_ok!(register(net, 900, 901));
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
            let outgoing = U256::zero();
            assert_ok!(Commitments::set_commitment(
                RuntimeOrigin::signed(outgoing),
                net,
                Box::new(pallet_commitments::CommitmentInfo {
                    fields: vec![pallet_commitments::Data::TimelockEncrypted {
                        encrypted: vec![1, 2, 3].try_into().unwrap(),
                        reveal_round: u64::MAX,
                    }]
                    .try_into()
                    .unwrap(),
                }),
            ));
            Axons::<Test>::insert(net, outgoing, AxonInfoOf::default());
            Prometheus::<Test>::insert(net, outgoing, PrometheusInfoOf::default());
            NeuronCertificates::<Test>::insert(net, outgoing, NeuronCertificateOf::default());
            let metadata = || {
                (
                    Commitments::commitment_of(net, outgoing),
                    Commitments::last_commitment(net, outgoing),
                    Commitments::timelocked_index(),
                    Axons::<Test>::get(net, outgoing),
                    Prometheus::<Test>::get(net, outgoing),
                    NeuronCertificates::<Test>::get(net, outgoing),
                )
            };
            let before = metadata();
            assert!(Commitments::timelocked_index().contains(&(net, outgoing)));
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
            assert!(NullMiners::<Test>::contains_key(net, outgoing));
            assert_eq!(metadata(), before);
            assert!(Commitments::commitment_of(net, successor).is_none());
            assert!(!Axons::<Test>::contains_key(net, successor));
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

#[test]
fn null_hotkey_swap_moves_registry_rewards_and_endpoints() {
    for all_subnets in [false, true] {
        for keep_stake in [false, true] {
            for null_enabled in [false, true] {
                for yuma_member in [false, true] {
                    new_test_ext(1).execute_with(|| {
                        let net = setup(1);
                        let other = NetUid::from(2);
                        let old = U256::from(900);
                        let new = U256::from(902);
                        let cold = U256::from(901);
                        assert_ok!(register(net, 900, 901));
                        assert_ok!(SubtensorModule::create_account_if_non_existent(&cold, &old));
                        if yuma_member {
                            SubtensorModule::append_neuron(net, &old, 1);
                        }
                        SubtensorModule::init_new_network(other, 360);
                        assert_ok!(SubtensorModule::do_set_null_consensus(other, true));
                        assert_ok!(SubtensorModule::enroll_null_miner(other, &old, &cold));
                        ServingRateLimit::<Test>::insert(net, 0);
                        let serve = |key| {
                            SubtensorModule::do_serve_axon(
                                RuntimeOrigin::signed(key),
                                net,
                                1,
                                0x08080808,
                                8080,
                                4,
                                0,
                                0,
                                0,
                                Some(b"CERT".to_vec()),
                            )
                        };
                        let prometheus = |key| {
                            SubtensorModule::do_serve_prometheus(
                                RuntimeOrigin::signed(key),
                                net,
                                1,
                                0x08080808,
                                9090,
                                4,
                            )
                        };
                        assert_ok!(serve(old));
                        assert_ok!(prometheus(old));
                        let axon = Axons::<Test>::get(net, old);
                        let prom = Prometheus::<Test>::get(net, old);
                        let cert = NeuronCertificates::<Test>::get(net, old);
                        assert!(cert.is_some());
                        NullMinerOwnerGeneration::<Test>::insert(net, old, 7);
                        SubtensorModule::accrue_null_rewards(net, 200u64.into());
                        let miner = NullMiners::<Test>::get(net, old);
                        let uid = NullMinerUids::<Test>::get(net, old).unwrap();
                        assert_ok!(SubtensorModule::do_set_null_consensus(net, null_enabled));
                        add_balance_to_coldkey_account(&cold, 1_000_000_000_000u64.into());
                        assert_ok!(SubtensorModule::do_swap_hotkey(
                            RuntimeOrigin::signed(cold),
                            &old,
                            &new,
                            if all_subnets { None } else { Some(net) },
                            keep_stake,
                        ));
                        assert!(!NullMiners::<Test>::contains_key(net, old));
                        assert!(!NullMinerUids::<Test>::contains_key(net, old));
                        assert!(!NullMinerOwnerGeneration::<Test>::contains_key(net, old));
                        assert_eq!(NullMiners::<Test>::get(net, new), miner);
                        assert_eq!(NullMinerKeys::<Test>::get(net, uid), Some(new));
                        assert_eq!(NullMinerUids::<Test>::get(net, new), Some(uid));
                        assert_eq!(NullMinerOwnerGeneration::<Test>::get(net, new), 7);
                        assert_eq!(NullMinerCount::<Test>::get(net), 2);
                        assert_eq!(Axons::<Test>::get(net, new), axon);
                        assert_eq!(Prometheus::<Test>::get(net, new), prom);
                        assert_eq!(NeuronCertificates::<Test>::get(net, new), cert);
                        assert!(!Axons::<Test>::contains_key(net, old));
                        assert!(!Prometheus::<Test>::contains_key(net, old));
                        assert!(!NeuronCertificates::<Test>::contains_key(net, old));
                        assert_eq!(NullMiners::<Test>::contains_key(other, new), all_subnets);
                        assert_eq!(NullMiners::<Test>::contains_key(other, old), !all_subnets);
                        assert_ok!(SubtensorModule::do_set_null_consensus(net, true));
                        assert_noop!(serve(old), Error::<Test>::HotKeyNotRegisteredInNetwork);
                        assert_noop!(prometheus(old), Error::<Test>::HotKeyNotRegisteredInNetwork);
                        assert_ok!(serve(new));
                        assert_ok!(prometheus(new));
                        assert_noop!(claim(net, 900, 901), Error::<Test>::NullMinerNotRegistered);
                        assert_eq!(pending(net, 902), 100);
                        assert_ok!(claim(net, 902, 901));
                        assert_eq!(pending(net, 902), 0);
                        assert_eq!(LastHotkeySwapOnNetuid::<Test>::get(net, cold), 2);
                    });
                }
            }
        }
    }
}

#[test]
fn null_hotkey_swap_rejects_registered_destination_without_charging() {
    for all_subnets in [false, true] {
        new_test_ext(1).execute_with(|| {
            let net = setup(1);
            let old = U256::from(900);
            let new = U256::from(902);
            let cold = U256::from(901);
            assert_ok!(register(net, 900, 901));
            assert_ok!(SubtensorModule::create_account_if_non_existent(&cold, &old));
            assert_ok!(register(net, 902, 901));
            // Dormant null registrations must also be protected from overwrites.
            assert_ok!(SubtensorModule::do_set_null_consensus(net, false));
            add_balance_to_coldkey_account(&cold, 1_000_000_000_000u64.into());
            let before = sp_io::storage::root(sp_runtime::StateVersion::V1);
            let result = SubtensorModule::do_swap_hotkey(
                RuntimeOrigin::signed(cold),
                &old,
                &new,
                if all_subnets { None } else { Some(net) },
                false,
            );
            assert_eq!(
                result.unwrap_err().error,
                Error::<Test>::HotKeyAlreadyRegisteredInSubNet.into()
            );
            assert_eq!(sp_io::storage::root(sp_runtime::StateVersion::V1), before);
        });
    }
}

#[test]
fn null_hotkey_swap_preserves_u64_uid_without_scanning_miners() {
    new_test_ext(1).execute_with(|| {
        let net = setup(1);
        let uid = 1u64 << 40;
        NullMinerCount::<Test>::insert(net, uid);
        assert_ok!(register(net, 900, 901));
        let cold = U256::from(901);
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &cold,
            &U256::from(900)
        ));
        add_balance_to_coldkey_account(&cold, 1_000_000_000_000u64.into());
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(cold),
            &U256::from(900),
            &U256::from(902),
            Some(net),
            false,
        ));
        assert_eq!(NullMinerCount::<Test>::get(net), uid + 1);
        assert_eq!(NullMinerKeys::<Test>::get(net, uid), Some(U256::from(902)));
        assert_eq!(NullMinerUids::<Test>::get(net, U256::from(902)), Some(uid));
    });
}

#[test]
fn null_uid_indexes_are_removed_during_subnet_cleanup() {
    new_test_ext(1).execute_with(|| {
        let net = setup(1);
        assert_ok!(register(net, 900, 901));
        let mut meter = frame_support::weights::WeightMeter::new();
        assert!(SubtensorModule::remove_network_map_parameters(
            net, &mut meter
        ));
        assert_eq!(NullMinerKeys::<Test>::iter_prefix(net).count(), 0);
        assert_eq!(NullMinerUids::<Test>::iter_prefix(net).count(), 0);
    });
}

#[test]
fn null_hotkey_swap_cannot_move_another_coldkeys_null_identity() {
    new_test_ext(1).execute_with(|| {
        let net = setup(1);
        assert_ok!(register(net, 900, 901));
        let staking_owner = U256::from(999);
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &staking_owner,
            &U256::from(900)
        ));
        add_balance_to_coldkey_account(&staking_owner, 1_000_000_000_000u64.into());
        let before = sp_io::storage::root(sp_runtime::StateVersion::V1);
        let result = SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(staking_owner),
            &U256::from(900),
            &U256::from(902),
            Some(net),
            false,
        );
        assert_eq!(
            result.unwrap_err().error,
            Error::<Test>::NonAssociatedColdKey.into()
        );
        assert_eq!(sp_io::storage::root(sp_runtime::StateVersion::V1), before);
    });
}

#[test]
fn null_hotkey_swap_follows_coldkey_lineage() {
    new_test_ext(1).execute_with(|| {
        let net = setup(1);
        assert_ok!(register(net, 900, 901));
        let successor = U256::from(903);
        SubtensorModule::record_coldkey_swap_lineage(&U256::from(901), &successor);
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &successor,
            &U256::from(900)
        ));
        add_balance_to_coldkey_account(&successor, 1_000_000_000_000u64.into());
        SubtensorModule::accrue_null_rewards(net, 200u64.into());
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(successor),
            &U256::from(900),
            &U256::from(902),
            Some(net),
            false,
        ));
        assert_eq!(
            NullMiners::<Test>::get(net, U256::from(902)).unwrap().0,
            successor
        );
        assert_ok!(claim(net, 902, 903));
    });
}

#[test]
fn null_rotation_does_not_depend_on_global_staking_ownership() {
    for all_subnets in [false, true] {
        for squatted in [false, true] {
            for paused in [false, true] {
                new_test_ext(1).execute_with(|| {
                    let net = setup(1);
                    let old = U256::from(900);
                    let new = U256::from(902);
                    let cold = U256::from(901);
                    let third_party = U256::from(999);
                    assert_ok!(register(net, 900, 901));
                    assert!(!Owner::<Test>::contains_key(old));
                    assert!(OwnedHotkeys::<Test>::get(cold).is_empty());
                    if squatted {
                        assert_ok!(SubtensorModule::do_try_associate_hotkey(&third_party, &old));
                        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
                            &old,
                            &third_party,
                            net,
                            10u64.into(),
                        );
                        // A third party's Yuma association cannot confer null serving rights.
                        SubtensorModule::append_neuron(net, &old, 1);
                    }
                    let ownership = (
                        Owner::<Test>::get(old),
                        Owner::<Test>::get(new),
                        OwnedHotkeys::<Test>::get(third_party),
                        StakingHotkeys::<Test>::get(third_party),
                    );
                    let stake = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                        &old,
                        &third_party,
                        net,
                    );
                    ServingRateLimit::<Test>::insert(net, 0);
                    let serve = |hotkey| {
                        SubtensorModule::do_serve_axon(
                            RuntimeOrigin::signed(hotkey),
                            net,
                            1,
                            0x08080808,
                            8080,
                            4,
                            0,
                            0,
                            0,
                            Some(b"CERT".to_vec()),
                        )
                    };
                    assert_ok!(serve(old));
                    let old_axon = Axons::<Test>::get(net, old);
                    SubtensorModule::accrue_null_rewards(net, 200u64.into());
                    if paused {
                        assert_ok!(SubtensorModule::do_set_null_consensus(net, false));
                    }
                    add_balance_to_coldkey_account(&cold, 1_000_000_000_000u64.into());
                    assert_ok!(SubtensorModule::do_swap_hotkey(
                        RuntimeOrigin::signed(cold),
                        &old,
                        &new,
                        if all_subnets { None } else { Some(net) },
                        false,
                    ));
                    assert_eq!(
                        ownership,
                        (
                            Owner::<Test>::get(old),
                            Owner::<Test>::get(new),
                            OwnedHotkeys::<Test>::get(third_party),
                            StakingHotkeys::<Test>::get(third_party)
                        )
                    );
                    assert_eq!(
                        SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                            &old,
                            &third_party,
                            net
                        ),
                        stake
                    );
                    if squatted {
                        assert_eq!(Axons::<Test>::get(net, old), old_axon);
                    }
                    assert_eq!(NullMinerCount::<Test>::get(net), 2);
                    assert_eq!(NullMinerKeys::<Test>::get(net, 1), Some(new));
                    assert!(!NullMiners::<Test>::contains_key(net, old));
                    assert!(NeuronCertificates::<Test>::contains_key(net, new));
                    assert_eq!(pending(net, 902), 100);
                    assert_ok!(claim(net, 902, 901));
                    assert_ok!(SubtensorModule::do_set_null_consensus(net, true));
                    assert_noop!(serve(old), Error::<Test>::HotKeyNotRegisteredInNetwork);
                    assert_ok!(serve(new));
                });
            }
        }
    }
}

#[test]
fn null_rotation_after_coldkey_swap_needs_no_global_association() {
    new_test_ext(1).execute_with(|| {
        let net = setup(1);
        assert_ok!(register(net, 900, 901));
        let successor = U256::from(903);
        SubtensorModule::record_coldkey_swap_lineage(&U256::from(901), &successor);
        assert_ok!(SubtensorModule::do_try_associate_hotkey(
            &U256::from(999),
            &U256::from(900)
        ));
        add_balance_to_coldkey_account(&successor, 1_000_000_000_000u64.into());
        assert_ok!(SubtensorModule::do_swap_hotkey(
            RuntimeOrigin::signed(successor),
            &U256::from(900),
            &U256::from(902),
            Some(net),
            true
        ));
        assert_eq!(
            NullMiners::<Test>::get(net, U256::from(902)).unwrap().0,
            successor
        );
    });
}

#[test]
fn null_rotation_rejects_a_destination_owned_by_another_coldkey() {
    for netuid in [Some(NetUid::from(1)), None] {
        new_test_ext(1).execute_with(|| {
            let net = setup(1);
            assert_ok!(register(net, 900, 901));
            let victim = U256::from(902);
            let victim_owner = U256::from(903);
            assert_ok!(SubtensorModule::create_account_if_non_existent(
                &victim_owner,
                &victim
            ));
            add_balance_to_coldkey_account(&U256::from(901), 1_000_000_000_000u64.into());
            assert_noop!(
                SubtensorModule::do_swap_hotkey(
                    RuntimeOrigin::signed(U256::from(901)),
                    &U256::from(900),
                    &victim,
                    netuid,
                    false,
                ),
                Error::<Test>::NonAssociatedColdKey
            );
            // The failed attachment leaves the victim free to rotate globally.
            add_balance_to_coldkey_account(&victim_owner, 1_000_000_000_000u64.into());
            assert_ok!(SubtensorModule::do_swap_hotkey(
                RuntimeOrigin::signed(victim_owner),
                &victim,
                &U256::from(904),
                None,
                false,
            ));
            assert_eq!(Owner::<Test>::get(U256::from(904)), victim_owner);
        });
    }
}

#[test]
fn global_rotation_ignores_foreign_null_registrations_and_their_cooldowns() {
    for staking_owner in [false, true] {
        for paused in [false, true] {
            new_test_ext(1).execute_with(|| {
                let net = setup(1);
                assert_ok!(register(net, 900, 901));
                let other = NetUid::from(2);
                let old = U256::from(900);
                let new = U256::from(902);
                let cold = U256::from(903);
                // Both identities have lazy coldkey succession to exercise
                // ownership resolution without rewriting a foreign record.
                SubtensorModule::record_coldkey_swap_lineage(&U256::from(901), &cold);
                SubtensorModule::init_new_network(other, 360);
                assert_ok!(SubtensorModule::do_set_null_consensus(other, true));
                assert_ok!(SubtensorModule::enroll_null_miner(
                    other,
                    &old,
                    &U256::from(999)
                ));
                SubtensorModule::record_coldkey_swap_lineage(&U256::from(999), &U256::from(998));
                let certificate = NeuronCertificateOf::try_from(b"FOREIGN".to_vec()).unwrap();
                NeuronCertificates::<Test>::insert(other, old, &certificate);
                SubtensorModule::accrue_null_rewards(other, 20u64.into());
                let foreign = NullMiners::<Test>::get(other, old);
                let generation = NullMinerOwnerGeneration::<Test>::get(other, old);
                let last = SubtensorModule::get_current_block_as_u64();
                LastHotkeySwapOnNetuid::<Test>::insert(other, cold, last);
                if staking_owner {
                    assert_ok!(SubtensorModule::create_account_if_non_existent(&cold, &old));
                    SubtensorModule::append_neuron(net, &old, 1);
                }
                if paused {
                    assert_ok!(SubtensorModule::do_set_null_consensus(other, false));
                }
                add_balance_to_coldkey_account(&cold, 1_000_000_000_000u64.into());
                assert_ok!(SubtensorModule::do_swap_hotkey(
                    RuntimeOrigin::signed(cold),
                    &old,
                    &new,
                    None,
                    false,
                ));
                assert!(!NullMiners::<Test>::contains_key(net, old));
                assert_eq!(NullMiners::<Test>::get(net, new).unwrap().0, cold);
                assert_eq!(NullMiners::<Test>::get(other, old), foreign);
                assert!(!NullMiners::<Test>::contains_key(other, new));
                assert_eq!(
                    NullMinerOwnerGeneration::<Test>::get(other, old),
                    generation
                );
                assert_eq!(NullMinerKeys::<Test>::get(other, 0), Some(old));
                assert_eq!(NullMinerUids::<Test>::get(other, old), Some(0));
                assert_eq!(LastHotkeySwapOnNetuid::<Test>::get(other, cold), last);
                assert_eq!(
                    NeuronCertificates::<Test>::get(other, old),
                    Some(certificate)
                );
                assert!(!NeuronCertificates::<Test>::contains_key(other, new));
                if staking_owner {
                    assert_eq!(Owner::<Test>::get(new), cold);
                    assert!(SubtensorModule::is_hotkey_registered_on_network(net, &new));
                }
                assert_ok!(claim(other, 900, 998));
            });
        }
    }
}

#[test]
fn global_yuma_rotation_preserves_retained_null_endpoints() {
    for paused in [false, true] {
        for keep_stake in [false, true] {
            new_test_ext(1).execute_with(|| {
                let net = setup(1);
                let old = U256::from(900);
                let new = U256::from(902);
                let cold = U256::from(901);
                assert_ok!(register(net, 900, 999));
                SubtensorModule::record_coldkey_swap_lineage(&U256::from(999), &U256::from(998));
                assert_ok!(SubtensorModule::create_account_if_non_existent(&cold, &old));
                SubtensorModule::append_neuron(net, &old, 1);
                let uid = SubtensorModule::get_uid_for_net_and_hotkey(net, &old).unwrap();
                ServingRateLimit::<Test>::insert(net, 0);
                let serve_axon = |hotkey| {
                    SubtensorModule::do_serve_axon(
                        RuntimeOrigin::signed(hotkey),
                        net,
                        1,
                        0x08080808,
                        8080,
                        4,
                        0,
                        0,
                        0,
                        Some(b"FOREIGN".to_vec()),
                    )
                };
                let serve_prometheus = |hotkey| {
                    SubtensorModule::do_serve_prometheus(
                        RuntimeOrigin::signed(hotkey),
                        net,
                        1,
                        0x08080808,
                        9090,
                        4,
                    )
                };
                assert_ok!(serve_axon(old));
                assert_ok!(serve_prometheus(old));
                let endpoints = |hotkey| {
                    (
                        Axons::<Test>::get(net, hotkey),
                        Prometheus::<Test>::get(net, hotkey),
                        NeuronCertificates::<Test>::get(net, hotkey),
                    )
                };
                let before = endpoints(old);
                assert!(before.0.is_some() && before.1.is_some() && before.2.is_some());
                let miner = NullMiners::<Test>::get(net, old);
                SubtensorModule::accrue_null_rewards(net, 20u64.into());
                if paused {
                    assert_ok!(SubtensorModule::do_set_null_consensus(net, false));
                }
                add_balance_to_coldkey_account(&cold, 1_000_000_000_000u64.into());
                assert_ok!(SubtensorModule::do_swap_hotkey(
                    RuntimeOrigin::signed(cold),
                    &old,
                    &new,
                    None,
                    keep_stake,
                ));
                assert_eq!(endpoints(old), before);
                assert_eq!(endpoints(new), before);
                assert_eq!(NullMiners::<Test>::get(net, old), miner);
                assert!(!NullMiners::<Test>::contains_key(net, new));
                assert_eq!(NullMinerKeys::<Test>::get(net, 1), Some(old));
                assert_eq!(NullMinerUids::<Test>::get(net, old), Some(1));
                assert_eq!(
                    SubtensorModule::get_uid_for_net_and_hotkey(net, &new),
                    Ok(uid)
                );
                assert!(!SubtensorModule::is_hotkey_registered_on_network(net, &old));
                assert_ok!(SubtensorModule::do_set_null_consensus(net, true));
                assert_ok!(serve_axon(old));
                assert_ok!(serve_prometheus(old));
                assert_noop!(serve_axon(new), Error::<Test>::HotKeyNotRegisteredInNetwork);
                assert_noop!(
                    serve_prometheus(new),
                    Error::<Test>::HotKeyNotRegisteredInNetwork
                );
                assert_ok!(claim(net, 900, 998));
            });
        }
    }
}

#[test]
fn enabling_null_clears_only_its_pending_encrypted_submissions() {
    new_test_ext(1).execute_with(|| {
        let net = setup(1);
        assert_ok!(SubtensorModule::do_set_null_consensus(net, false));
        let index = SubtensorModule::get_mechanism_storage_index(net, 0.into());
        let other = NetUid::from(2);
        SubtensorModule::init_new_network(other, 360);
        let other_index = SubtensorModule::get_mechanism_storage_index(other, 0.into());
        let commits = std::collections::VecDeque::from([(
            U256::from(1),
            1,
            vec![1, 2, 3].try_into().unwrap(),
            1000,
        )]);
        // Cover every retained epoch bucket, including same-block writes.
        for epoch in 0..MAX_COMMIT_REVEAL_PEROIDS + 2 {
            TimelockedWeightCommits::<Test>::insert(index, epoch, &commits);
        }
        TimelockedWeightCommits::<Test>::insert(other_index, 0, &commits);
        Weights::<Test>::insert(index, 0, vec![(0, u16::MAX)]);
        SubtensorModule::set_commit_reveal_weights_enabled(net, true);

        assert_ok!(SubtensorModule::do_set_null_consensus(net, true));
        assert!(
            TimelockedWeightCommits::<Test>::iter_key_prefix(index)
                .next()
                .is_none()
        );
        assert_eq!(
            TimelockedWeightCommits::<Test>::get(other_index, 0),
            commits
        );
        assert_ok!(SubtensorModule::reveal_crv3_commits_for_subnet(net));
        assert_ok!(SubtensorModule::do_set_null_consensus(net, false));
        assert_eq!(Weights::<Test>::get(index, 0), vec![(0, u16::MAX)]);
        assert!(SubtensorModule::get_commit_reveal_weights_enabled(net));
        assert!(
            TimelockedWeightCommits::<Test>::iter_key_prefix(index)
                .next()
                .is_none()
        );
    });
}

#[test]
fn failed_null_enable_preserves_pending_encrypted_submissions() {
    new_test_ext(1).execute_with(|| {
        let net = NetUid::from(1);
        SubtensorModule::init_new_network(net, 360);
        let index = SubtensorModule::get_mechanism_storage_index(net, 0.into());
        TimelockedWeightCommits::<Test>::insert(
            index,
            0,
            std::collections::VecDeque::from([(
                U256::from(1),
                1,
                vec![1, 2, 3].try_into().unwrap(),
                1000,
            )]),
        );
        MechanismCountCurrent::<Test>::insert(net, MechId::from(2));
        assert_noop!(
            SubtensorModule::do_set_null_consensus(net, true),
            Error::<Test>::NullConsensusRequiresSingleMechanism
        );
        MechanismCountCurrent::<Test>::insert(net, MechId::from(1));
        SubnetworkN::<Test>::insert(net, DefaultMaxAllowedUids::<Test>::get() + 1);
        assert_noop!(
            SubtensorModule::do_set_null_consensus(net, true),
            Error::<Test>::NullConsensusYumaCapacityExceeded
        );
    });
}

#[test]
fn null_rotation_cannot_overwrite_a_yuma_destination() {
    new_test_ext(1).execute_with(|| {
        let net = setup(1);
        assert_ok!(register(net, 900, 901));
        let new = U256::from(902);
        SubtensorModule::append_neuron(net, &new, 1);
        let cold = U256::from(901);
        add_balance_to_coldkey_account(&cold, 1_000_000_000_000u64.into());
        assert_noop!(
            SubtensorModule::do_swap_hotkey(
                RuntimeOrigin::signed(cold),
                &U256::from(900),
                &new,
                Some(net),
                false
            ),
            Error::<Test>::HotKeyAlreadyRegisteredInSubNet
        );
    });
}

#[test]
fn null_rotation_without_swap_funds_rolls_back_registry_and_endpoints() {
    new_test_ext(1).execute_with(|| {
        let net = setup(1);
        assert_ok!(register(net, 900, 901));
        assert_noop!(
            SubtensorModule::do_swap_hotkey(
                RuntimeOrigin::signed(U256::from(901)),
                &U256::from(900),
                &U256::from(902),
                Some(net),
                false
            ),
            Error::<Test>::NotEnoughBalanceToPaySwapHotKey
        );
    });
}
