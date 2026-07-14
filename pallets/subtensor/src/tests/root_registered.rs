#![allow(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::arithmetic_side_effects
)]

use super::mock::*;
use crate::root_registered::{EmaState, InFlightEmaSample, SampleStep};
use crate::*;
use frame_support::assert_ok;
use frame_support::weights::Weight;
use sp_core::U256;
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::{AlphaBalance, NetUid};

fn ref_count(coldkey: &U256) -> u32 {
    RootRegisteredHotkeyCount::<Test>::get(coldkey)
}

#[test]
fn ref_count_helpers_basic_behavior() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(7);

        // Reader on an unset key.
        assert_eq!(ref_count(&coldkey), 0);
        assert!(!SubtensorModule::coldkey_has_root_hotkey(&coldkey));

        // Saturating decrement at zero must not underflow.
        SubtensorModule::decrement_root_registered_hotkey_count(&coldkey);
        assert_eq!(ref_count(&coldkey), 0);
        assert!(!RootRegisteredHotkeyCount::<Test>::contains_key(coldkey));

        // Increment populates storage and flips the reader.
        SubtensorModule::increment_root_registered_hotkey_count(&coldkey);
        assert!(RootRegisteredHotkeyCount::<Test>::contains_key(coldkey));
        assert!(SubtensorModule::coldkey_has_root_hotkey(&coldkey));

        SubtensorModule::increment_root_registered_hotkey_count(&coldkey);
        assert_eq!(ref_count(&coldkey), 2);

        SubtensorModule::decrement_root_registered_hotkey_count(&coldkey);
        assert_eq!(ref_count(&coldkey), 1);

        // Decrement to zero removes the storage entry.
        SubtensorModule::decrement_root_registered_hotkey_count(&coldkey);
        assert!(!RootRegisteredHotkeyCount::<Test>::contains_key(coldkey));

        // Saturating decrement on an absent key must not resurrect the entry.
        SubtensorModule::decrement_root_registered_hotkey_count(&coldkey);
        assert!(!RootRegisteredHotkeyCount::<Test>::contains_key(coldkey));
    });
}

#[test]
fn ref_count_increment_fires_added_hook_only_on_zero_to_one_transition() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(10);
        let _ = take_root_registration_log();

        SubtensorModule::increment_root_registered_hotkey_count(&coldkey);
        assert_eq!(
            take_root_registration_log(),
            vec![RootRegistrationChange::Added(coldkey)]
        );

        // Subsequent increments stay above zero and must not re-fire.
        SubtensorModule::increment_root_registered_hotkey_count(&coldkey);
        SubtensorModule::increment_root_registered_hotkey_count(&coldkey);
        assert!(take_root_registration_log().is_empty());
    });
}

#[test]
fn ref_count_decrement_fires_removed_hook_only_on_one_to_zero_transition() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(10);
        SubtensorModule::increment_root_registered_hotkey_count(&coldkey);
        SubtensorModule::increment_root_registered_hotkey_count(&coldkey);
        SubtensorModule::increment_root_registered_hotkey_count(&coldkey);
        let _ = take_root_registration_log();

        // Above-zero decrements are silent.
        SubtensorModule::decrement_root_registered_hotkey_count(&coldkey);
        SubtensorModule::decrement_root_registered_hotkey_count(&coldkey);
        assert!(take_root_registration_log().is_empty());

        // The 1→0 edge fires once.
        SubtensorModule::decrement_root_registered_hotkey_count(&coldkey);
        assert_eq!(
            take_root_registration_log(),
            vec![RootRegistrationChange::Removed(coldkey)]
        );

        // Decrementing a zero count must not fire a spurious `Removed`.
        SubtensorModule::decrement_root_registered_hotkey_count(&coldkey);
        assert!(take_root_registration_log().is_empty());
    });
}

#[test]
fn ref_count_invariant_holds_across_mutations() {
    new_test_ext(1).execute_with(|| {
        let alpha = NetUid::from(1);
        add_network(NetUid::ROOT, 1, 0);
        add_network(alpha, 1, 0);

        // Lift the per-block / per-interval registration caps so the test
        // can register five hotkeys without stepping blocks.
        MaxRegistrationsPerBlock::<Test>::set(NetUid::ROOT, 64);
        TargetRegistrationsPerInterval::<Test>::set(NetUid::ROOT, 64);

        assert_ok!(SubtensorModule::check_root_registered_hotkey_count());

        let cold1 = U256::from(10);
        let cold2 = U256::from(20);
        let cold3 = U256::from(30);
        let h1 = U256::from(11);
        let h2 = U256::from(12);
        let h3 = U256::from(21);
        let h4 = U256::from(31);

        // Mix of registrations across multiple coldkeys.
        root_register_with_stake(&cold1, &h1, alpha);
        root_register_with_stake(&cold1, &h2, alpha);
        root_register_with_stake(&cold2, &h3, alpha);
        root_register_with_stake(&cold3, &h4, alpha);
        assert_ok!(SubtensorModule::check_root_registered_hotkey_count());

        // Replace path through `do_root_register` at the cap.
        MaxAllowedUids::<Test>::set(NetUid::ROOT, 4);
        let cold4 = U256::from(40);
        let h5 = U256::from(41);
        register_ok_neuron(alpha, h5, cold4, 0);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &h5,
            &cold4,
            NetUid::ROOT,
            AlphaBalance::from(10_000_000_000_u64),
        );
        assert_ok!(SubtensorModule::root_register(
            RuntimeOrigin::signed(cold4),
            h5,
        ));
        assert_ok!(SubtensorModule::check_root_registered_hotkey_count());

        // Coldkey swap moves a multi-hotkey holder's count to a fresh coldkey.
        let cold1_new = U256::from(99);
        assert_ok!(SubtensorModule::do_swap_coldkey(&cold1, &cold1_new));
        assert_ok!(SubtensorModule::check_root_registered_hotkey_count());

        // Trim drops the lowest emitter; tightens the invariant under
        // bulk removal.
        ImmunityPeriod::<Test>::set(NetUid::ROOT, 0);
        MinAllowedUids::<Test>::set(NetUid::ROOT, 1);
        assert_ok!(SubtensorModule::trim_to_max_allowed_uids(NetUid::ROOT, 1));
        assert_ok!(SubtensorModule::check_root_registered_hotkey_count());
    });
}

#[test]
fn ref_count_invariant_detects_stale_overcount() {
    new_test_ext(1).execute_with(|| {
        let alpha = NetUid::from(1);
        add_network(NetUid::ROOT, 1, 0);
        add_network(alpha, 1, 0);

        let coldkey = U256::from(10);
        root_register_with_stake(&coldkey, &U256::from(11), alpha);
        assert_ok!(SubtensorModule::check_root_registered_hotkey_count());

        // Simulate a buggy code path that incremented the counter without a
        // matching root registration. The invariant must surface the drift.
        SubtensorModule::increment_root_registered_hotkey_count(&coldkey);
        assert!(SubtensorModule::check_root_registered_hotkey_count().is_err());
    });
}

#[test]
fn ref_count_invariant_detects_missing_index_entry() {
    new_test_ext(1).execute_with(|| {
        let alpha = NetUid::from(1);
        add_network(NetUid::ROOT, 1, 0);
        add_network(alpha, 1, 0);

        let coldkey = U256::from(10);
        root_register_with_stake(&coldkey, &U256::from(11), alpha);
        assert_ok!(SubtensorModule::check_root_registered_hotkey_count());

        // Simulate a buggy path that registered a root hotkey without
        // updating the reverse index. The invariant must catch the
        // coldkey that now has root hotkeys but no counter entry.
        RootRegisteredHotkeyCount::<Test>::remove(coldkey);
        assert!(SubtensorModule::check_root_registered_hotkey_count().is_err());
    });
}

#[test]
fn inspector_invariant_passes_when_members_match_root_registered_coldkeys() {
    new_test_ext(1).execute_with(|| {
        let alpha = NetUid::from(1);
        add_network(NetUid::ROOT, 1, 0);
        add_network(alpha, 1, 0);
        MaxRegistrationsPerBlock::<Test>::set(NetUid::ROOT, 64);
        TargetRegistrationsPerInterval::<Test>::set(NetUid::ROOT, 64);

        let cold1 = U256::from(10);
        let cold2 = U256::from(20);
        // Two hotkeys under cold1, one under cold2: the expected root-registered
        // set is the two distinct coldkeys, not three.
        root_register_with_stake(&cold1, &U256::from(11), alpha);
        root_register_with_stake(&cold1, &U256::from(12), alpha);
        root_register_with_stake(&cold2, &U256::from(21), alpha);

        set_mock_root_registered_inspector_members(Some(vec![cold1, cold2]));
        assert_ok!(SubtensorModule::check_root_registered_matches_inspector());
    });
}

#[test]
fn inspector_invariant_skips_when_inspector_is_unset() {
    new_test_ext(1).execute_with(|| {
        let alpha = NetUid::from(1);
        add_network(NetUid::ROOT, 1, 0);
        add_network(alpha, 1, 0);
        root_register_with_stake(&U256::from(10), &U256::from(11), alpha);

        // Inspector unset by default: the check must silently no-op even
        // when the on-chain root set is non-empty.
        set_mock_root_registered_inspector_members(None);
        assert_ok!(SubtensorModule::check_root_registered_matches_inspector());
    });
}

#[test]
fn inspector_invariant_fails_when_members_differ_from_root_registered_coldkeys() {
    new_test_ext(1).execute_with(|| {
        let alpha = NetUid::from(1);
        add_network(NetUid::ROOT, 1, 0);
        add_network(alpha, 1, 0);

        let cold = U256::from(10);
        root_register_with_stake(&cold, &U256::from(11), alpha);

        // Inspector forgot to include the root-registered coldkey.
        set_mock_root_registered_inspector_members(Some(vec![]));
        assert!(SubtensorModule::check_root_registered_matches_inspector().is_err());

        // Inspector holds a coldkey that has no root hotkey.
        set_mock_root_registered_inspector_members(Some(vec![cold, U256::from(999)]));
        assert!(SubtensorModule::check_root_registered_matches_inspector().is_err());
    });
}

#[test]
fn ema_count_invariant_passes_when_ema_keys_match_root_registered_coldkeys() {
    new_test_ext(1).execute_with(|| {
        let alpha = NetUid::from(1);
        add_network(NetUid::ROOT, 1, 0);
        add_network(alpha, 1, 0);

        root_register_with_stake(&U256::from(10), &U256::from(11), alpha);

        assert_ok!(SubtensorModule::check_root_registered_ema_matches_count());
    });
}

#[test]
fn ema_count_invariant_detects_missing_ema_entry_for_registered_coldkey() {
    new_test_ext(1).execute_with(|| {
        let alpha = NetUid::from(1);
        add_network(NetUid::ROOT, 1, 0);
        add_network(alpha, 1, 0);

        let coldkey = U256::from(10);
        root_register_with_stake(&coldkey, &U256::from(11), alpha);
        RootRegisteredEma::<Test>::remove(coldkey);

        assert!(SubtensorModule::check_root_registered_ema_matches_count().is_err());
    });
}

#[test]
fn ema_count_invariant_detects_stale_ema_entry_for_unregistered_coldkey() {
    new_test_ext(1).execute_with(|| {
        let stale = U256::from(99);
        RootRegisteredEma::<Test>::insert(stale, EmaState::default());

        assert!(SubtensorModule::check_root_registered_ema_matches_count().is_err());
    });
}

#[test]
fn ema_slot_is_initialized_cleared_and_reinitialized_on_reentry() {
    new_test_ext(1).execute_with(|| {
        let alpha = NetUid::from(1);
        add_network(NetUid::ROOT, 1, 0);
        add_network(alpha, 1, 0);

        let coldkey = U256::from(10);
        assert!(!RootRegisteredEma::<Test>::contains_key(coldkey));

        // First root registration seeds a zero-valued slot.
        root_register_with_stake(&coldkey, &U256::from(11), alpha);
        let state = RootRegisteredEma::<Test>::get(coldkey);
        assert_eq!(state.ema, U64F64::from_num(0));
        assert_eq!(state.samples, 0);

        // The default mock provider completes a sample per tick, so two
        // ticks land two samples on the only registered coldkey.
        SubtensorModule::tick_root_registered_ema();
        SubtensorModule::tick_root_registered_ema();
        assert_eq!(RootRegisteredEma::<Test>::get(coldkey).samples, 2);

        // Drop to zero hotkeys: the EMA slot is cleared.
        SubtensorModule::decrement_root_registered_hotkey_count(&coldkey);
        assert!(!RootRegisteredEma::<Test>::contains_key(coldkey));

        // Re-register: state starts fresh.
        root_register_with_stake(&coldkey, &U256::from(12), alpha);
        let state = RootRegisteredEma::<Test>::get(coldkey);
        assert_eq!(state.ema, U64F64::from_num(0));
        assert_eq!(state.samples, 0);
    });
}

#[test]
fn ema_tick_blends_completed_sample_with_fixed_alpha() {
    new_test_ext(1).execute_with(|| {
        let alpha = NetUid::from(1);
        add_network(NetUid::ROOT, 1, 0);
        add_network(alpha, 1, 0);

        let coldkey = U256::from(10);
        root_register_with_stake(&coldkey, &U256::from(11), alpha);

        let _step = EmaValueProviderStepGuard::new(Some(|_, _| {
            (
                SampleStep::Complete {
                    sample: U64F64::from_num(100),
                },
                Weight::zero(),
            )
        }));

        SubtensorModule::tick_root_registered_ema();

        let state = RootRegisteredEma::<Test>::get(coldkey);
        let expected = U64F64::from_num(2)
            .saturating_div(U64F64::from_num(100))
            .saturating_mul(U64F64::from_num(100));
        assert_eq!(state.ema, expected);
        assert_eq!(state.samples, 1);
    });
}

#[test]
fn ema_tick_finalizes_samples_and_advances_cursor() {
    new_test_ext(1).execute_with(|| {
        let alpha = NetUid::from(1);
        add_network(NetUid::ROOT, 1, 0);
        add_network(alpha, 1, 0);
        MaxRegistrationsPerBlock::<Test>::set(NetUid::ROOT, 64);
        TargetRegistrationsPerInterval::<Test>::set(NetUid::ROOT, 64);

        let cold_a = U256::from(10);
        let cold_b = U256::from(20);
        root_register_with_stake(&cold_a, &U256::from(11), alpha);
        root_register_with_stake(&cold_b, &U256::from(21), alpha);
        let _ = take_ema_value_provider_log();

        // Default mock progress is single-shot; provider returns 42 as
        // the raw sample and the pallet blends it into the EMA.
        let _step = EmaValueProviderStepGuard::new(Some(|_, _| {
            (
                SampleStep::Complete {
                    sample: U64F64::from_num(42),
                },
                Weight::zero(),
            )
        }));

        // Two consecutive ticks: each finalizes a distinct member and
        // the cursor advances by one per finalize.
        assert_eq!(EmaSamplerState::<Test>::get().0, 0);
        SubtensorModule::tick_root_registered_ema();
        SubtensorModule::tick_root_registered_ema();

        let log = take_ema_value_provider_log();
        let touched: Vec<U256> = log.iter().map(|(k, _)| *k).collect();
        assert_eq!(touched.len(), 2);
        assert!(touched.contains(&cold_a) && touched.contains(&cold_b));

        let state_a = RootRegisteredEma::<Test>::get(cold_a);
        assert!(state_a.ema > U64F64::from_num(0));
        assert_eq!(state_a.samples, 1);
        let state_b = RootRegisteredEma::<Test>::get(cold_b);
        assert!(state_b.ema > U64F64::from_num(0));
        assert_eq!(state_b.samples, 1);

        // The cursor wraps and rebuilds the snapshot, so a third tick
        // revisits one of the members and bumps its counter to 2.
        SubtensorModule::tick_root_registered_ema();
        let revisited_samples = RootRegisteredEma::<Test>::get(cold_a).samples
            + RootRegisteredEma::<Test>::get(cold_b).samples;
        assert_eq!(revisited_samples, 3);
    });
}

#[test]
fn ema_tick_is_no_op_when_no_members() {
    new_test_ext(1).execute_with(|| {
        // No registrations: the rebuild produces an empty snapshot and
        // the tick must not touch the cursor or the provider log.
        let _ = take_ema_value_provider_log();
        let cursor_before = EmaSamplerState::<Test>::get().0;
        SubtensorModule::tick_root_registered_ema();
        assert_eq!(EmaSamplerState::<Test>::get().0, cursor_before);
        assert!(take_ema_value_provider_log().is_empty());
    });
}

#[test]
fn ema_tick_returns_weight_including_provider_contribution() {
    new_test_ext(1).execute_with(|| {
        let alpha = NetUid::from(1);
        add_network(NetUid::ROOT, 1, 0);
        add_network(alpha, 1, 0);
        root_register_with_stake(&U256::from(10), &U256::from(11), alpha);

        // Provider reports a non-zero per-step weight; the tick must
        // surface it through its return value so `on_initialize` can
        // bill the actual cost.
        let _step_weight = EmaValueProviderStepWeightGuard::new(Weight::from_parts(12_345, 0));
        let on_tick = SubtensorModule::tick_root_registered_ema();
        assert!(
            on_tick.ref_time() >= 12_345,
            "tick weight must include provider contribution, got {on_tick:?}"
        );
    });
}

#[test]
fn ema_tick_default_provider_advances_sample_count_without_changing_zero_ema() {
    new_test_ext(1).execute_with(|| {
        let alpha = NetUid::from(1);
        add_network(NetUid::ROOT, 1, 0);
        add_network(alpha, 1, 0);

        let coldkey = U256::from(10);
        root_register_with_stake(&coldkey, &U256::from(11), alpha);

        // No guards: MockEmaValueProvider's default step is single-shot done
        // with no contribution; finalize returns `previous.ema`. The EMA
        // stays at the init value (0) but the sample counter advances.
        let _ = take_ema_value_provider_log();
        SubtensorModule::tick_root_registered_ema();

        let state = RootRegisteredEma::<Test>::get(coldkey);
        assert_eq!(state.ema, U64F64::from_num(0));
        assert_eq!(state.samples, 1);
    });
}

#[test]
fn ema_tick_persists_provider_progress_until_sample_completes() {
    new_test_ext(1).execute_with(|| {
        let alpha = NetUid::from(1);
        add_network(NetUid::ROOT, 1, 0);
        add_network(alpha, 1, 0);

        let coldkey = U256::from(10);
        root_register_with_stake(&coldkey, &U256::from(11), alpha);

        // Step adds 100 per call and signals done only when offset
        // reaches 3 (i.e. after three chunks).
        let _step = EmaValueProviderStepGuard::new(Some(|_, mut progress| {
            progress.offset = progress.offset.saturating_add(1);
            progress.partial = progress.partial.saturating_add(100);
            if progress.offset >= 3 {
                (
                    SampleStep::Complete {
                        sample: U64F64::from_num(progress.partial as u64),
                    },
                    Weight::zero(),
                )
            } else {
                (SampleStep::Continue { progress }, Weight::zero())
            }
        }));

        // First two ticks accumulate partial state without finalizing.
        SubtensorModule::tick_root_registered_ema();
        let (cursor, progress) = EmaSamplerState::<Test>::get();
        assert_eq!(cursor, 0);
        let in_flight = progress.expect("mid-sample progress must be Some");
        assert_eq!(in_flight.progress.offset, 1);
        assert_eq!(in_flight.progress.partial, 100);
        assert_eq!(RootRegisteredEma::<Test>::get(coldkey).samples, 0);

        SubtensorModule::tick_root_registered_ema();
        let (cursor, progress) = EmaSamplerState::<Test>::get();
        assert_eq!(cursor, 0);
        let in_flight = progress.expect("mid-sample progress must be Some");
        assert_eq!(in_flight.progress.offset, 2);
        assert_eq!(in_flight.progress.partial, 200);
        assert_eq!(RootRegisteredEma::<Test>::get(coldkey).samples, 0);

        // Third tick finalizes: the accumulated 300 sample is blended
        // into the EMA, sample counter increments, progress resets, and
        // cursor advances.
        SubtensorModule::tick_root_registered_ema();
        let ema = RootRegisteredEma::<Test>::get(coldkey);
        assert!(ema.ema > U64F64::from_num(0));
        assert!(ema.ema < U64F64::from_num(300u64));
        assert_eq!(ema.samples, 1);
        let (cursor, progress) = EmaSamplerState::<Test>::get();
        assert_eq!(cursor, 1);
        assert!(progress.is_none());
    });
}

#[test]
fn ema_in_flight_progress_is_cleared_when_sampled_coldkey_leaves() {
    new_test_ext(1).execute_with(|| {
        let alpha = NetUid::from(1);
        add_network(NetUid::ROOT, 1, 0);
        add_network(alpha, 1, 0);

        let coldkey = U256::from(10);
        root_register_with_stake(&coldkey, &U256::from(11), alpha);

        let _step = EmaValueProviderStepGuard::new(Some(|_, mut progress| {
            progress.offset = progress.offset.saturating_add(1);
            progress.partial = progress.partial.saturating_add(100);
            (SampleStep::Continue { progress }, Weight::zero())
        }));

        SubtensorModule::tick_root_registered_ema();
        assert!(EmaSamplerState::<Test>::get().1.is_some());

        SubtensorModule::decrement_root_registered_hotkey_count(&coldkey);
        assert!(
            EmaSamplerState::<Test>::get().1.is_none(),
            "leaving the root-registered set must clear stale in-flight EMA progress"
        );

        SubtensorModule::increment_root_registered_hotkey_count(&coldkey);
        SubtensorModule::tick_root_registered_ema();
        let (_, progress) = EmaSamplerState::<Test>::get();
        let progress = progress.expect("fresh re-entry starts a new in-flight sample");
        assert_eq!(progress.progress.offset, 1);
        assert_eq!(progress.progress.partial, 100);
    });
}

#[test]
fn ema_in_flight_progress_survives_when_different_coldkey_leaves() {
    new_test_ext(1).execute_with(|| {
        let alpha = NetUid::from(1);
        add_network(NetUid::ROOT, 1, 0);
        add_network(alpha, 1, 0);
        MaxRegistrationsPerBlock::<Test>::set(NetUid::ROOT, 64);
        TargetRegistrationsPerInterval::<Test>::set(NetUid::ROOT, 64);

        let cold_a = U256::from(10);
        let cold_b = U256::from(20);
        root_register_with_stake(&cold_a, &U256::from(11), alpha);
        root_register_with_stake(&cold_b, &U256::from(21), alpha);

        let _step = EmaValueProviderStepGuard::new(Some(|_, mut progress| {
            progress.offset = progress.offset.saturating_add(1);
            progress.partial = progress.partial.saturating_add(100);
            (SampleStep::Continue { progress }, Weight::zero())
        }));

        SubtensorModule::tick_root_registered_ema();
        let (_, progress) = EmaSamplerState::<Test>::get();
        let in_flight = progress.expect("first tick must start an in-flight sample");
        let sampled = in_flight.coldkey;
        let other = if sampled == cold_a { cold_b } else { cold_a };

        SubtensorModule::decrement_root_registered_hotkey_count(&other);

        let (_, progress) = EmaSamplerState::<Test>::get();
        let progress = progress.expect("unrelated coldkey removal must not clear progress");
        assert_eq!(progress.coldkey, sampled);
        assert_eq!(progress.progress.offset, 1);
        assert_eq!(progress.progress.partial, 100);
    });
}

#[test]
fn ema_tick_discards_stale_in_flight_progress_for_wrong_coldkey() {
    new_test_ext(1).execute_with(|| {
        let alpha = NetUid::from(1);
        add_network(NetUid::ROOT, 1, 0);
        add_network(alpha, 1, 0);

        let coldkey = U256::from(10);
        let stale_coldkey = U256::from(20);
        root_register_with_stake(&coldkey, &U256::from(11), alpha);

        CurrentCycleMembers::<Test>::put(
            BoundedVec::try_from(vec![coldkey]).expect("one member fits snapshot bound"),
        );
        EmaSamplerState::<Test>::put((
            0,
            Some(InFlightEmaSample {
                coldkey: stale_coldkey,
                progress: MockEmaProgress {
                    offset: 99,
                    partial: 999,
                },
            }),
        ));

        let _step = EmaValueProviderStepGuard::new(Some(|_, progress| {
            assert_eq!(progress, MockEmaProgress::default());
            (SampleStep::Continue { progress }, Weight::zero())
        }));

        SubtensorModule::tick_root_registered_ema();

        let (_, progress) = EmaSamplerState::<Test>::get();
        let progress = progress.expect("continued sample must store fresh progress");
        assert_eq!(progress.coldkey, coldkey);
        assert_eq!(progress.progress, MockEmaProgress::default());
    });
}

#[test]
fn ema_tick_ignores_joined_coldkey_until_cycle_snapshot_rebuilds() {
    new_test_ext(1).execute_with(|| {
        let alpha = NetUid::from(1);
        add_network(NetUid::ROOT, 1, 0);
        add_network(alpha, 1, 0);
        MaxRegistrationsPerBlock::<Test>::set(NetUid::ROOT, 64);
        TargetRegistrationsPerInterval::<Test>::set(NetUid::ROOT, 64);

        let cold_a = U256::from(10);
        let cold_b = U256::from(20);
        let cold_c = U256::from(30);
        root_register_with_stake(&cold_a, &U256::from(11), alpha);
        root_register_with_stake(&cold_b, &U256::from(21), alpha);

        SubtensorModule::tick_root_registered_ema();
        let first_snapshot = CurrentCycleMembers::<Test>::get();
        assert_eq!(first_snapshot.len(), 2);

        root_register_with_stake(&cold_c, &U256::from(31), alpha);
        assert!(!first_snapshot.contains(&cold_c));
        assert!(!CurrentCycleMembers::<Test>::get().contains(&cold_c));

        let _ = take_ema_value_provider_log();
        SubtensorModule::tick_root_registered_ema();
        let touched: Vec<U256> = take_ema_value_provider_log()
            .iter()
            .map(|(coldkey, _)| *coldkey)
            .collect();
        assert!(!touched.contains(&cold_c));

        SubtensorModule::tick_root_registered_ema();
        assert!(CurrentCycleMembers::<Test>::get().contains(&cold_c));
    });
}

#[test]
fn ema_tick_skips_removed_coldkey_from_existing_cycle_snapshot() {
    new_test_ext(1).execute_with(|| {
        let alpha = NetUid::from(1);
        add_network(NetUid::ROOT, 1, 0);
        add_network(alpha, 1, 0);
        MaxRegistrationsPerBlock::<Test>::set(NetUid::ROOT, 64);
        TargetRegistrationsPerInterval::<Test>::set(NetUid::ROOT, 64);

        let cold_a = U256::from(10);
        let cold_b = U256::from(20);
        root_register_with_stake(&cold_a, &U256::from(11), alpha);
        root_register_with_stake(&cold_b, &U256::from(21), alpha);
        let _ = take_ema_value_provider_log();

        // Snapshot built on first tick; finalize bumps samples on
        // whichever validator the cursor lands on.
        SubtensorModule::tick_root_registered_ema();

        // Identify the validator at the *next* cursor position and
        // unregister it before the next tick reaches them.
        let snapshot = CurrentCycleMembers::<Test>::get();
        let cursor = EmaSamplerState::<Test>::get().0;
        let next = snapshot
            .get(cursor as usize)
            .copied()
            .expect("cursor must point at a member after first tick");
        SubtensorModule::decrement_root_registered_hotkey_count(&next);
        assert!(!RootRegisteredEma::<Test>::contains_key(next));

        // The next tick lands on the unregistered coldkey, finds it
        // missing from RootRegisteredEma, advances the cursor, and
        // does not finalize.
        let _ = take_ema_value_provider_log();
        SubtensorModule::tick_root_registered_ema();
        assert_eq!(EmaSamplerState::<Test>::get().0, cursor + 1);
        assert!(take_ema_value_provider_log().is_empty());
        assert!(!RootRegisteredEma::<Test>::contains_key(next));
    });
}
