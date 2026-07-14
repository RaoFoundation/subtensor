#![allow(
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing
)]

use super::*;
use crate::mock::*;
use frame_support::{assert_noop, assert_ok};
use sp_core::U256;
use sp_runtime::DispatchError;
use subtensor_runtime_common::Polls;

#[test]
fn environment_is_initialized() {
    TestState::default().build_and_execute(|| {
        assert!(MemberSet::Single(CollectiveId::Proposers).contains(&U256::from(PROPOSER)));
        assert_eq!(MemberSet::Single(CollectiveId::Triumvirate).len(), 3);
    });
}

#[test]
fn submit_pass_or_fail_records_state_and_schedules_deadline_alarm() {
    TestState::default().build_and_execute(|| {
        let index = submit_on(TRACK_PASS_OR_FAIL, U256::from(PROPOSER));
        let now = current_block();

        assert_eq!(ReferendumCount::<Test>::get(), 1);
        assert_eq!(ActiveCount::<Test>::get(), 1);
        assert!(signed_tally_exists(index));
        assert_eq!(scheduler_alarm_block(index), Some(now + DECISION_PERIOD));
        assert!(Pallet::<Test>::next_task_dispatch_time(index).is_none());

        match status_of(index) {
            ReferendumStatus::Ongoing(info) => {
                assert_eq!(info.track, TRACK_PASS_OR_FAIL);
                assert_eq!(info.proposer, U256::from(PROPOSER));
                assert_eq!(info.submitted, now);
                assert!(matches!(info.proposal, Proposal::Action(_)));
            }
            _ => panic!("expected Ongoing"),
        }

        assert!(has_event(|e| matches!(
            e,
            Event::Submitted { index: i, track, proposer }
                if *i == index
                    && *track == TRACK_PASS_OR_FAIL
                    && *proposer == U256::from(PROPOSER)
        )));
    });
}

#[test]
fn submit_adjustable_schedules_enact_wrapper_at_initial_delay() {
    TestState::default().build_and_execute(|| {
        let index = submit_on(TRACK_ADJUSTABLE, U256::from(PROPOSER));
        let now = current_block();

        assert!(matches!(
            status_of(index),
            ReferendumStatus::Ongoing(ReferendumInfo {
                proposal: Proposal::Review,
                ..
            })
        ));
        assert_eq!(
            Pallet::<Test>::next_task_dispatch_time(index),
            Some(now + INITIAL_DELAY)
        );
        assert!(scheduler_alarm_block(index).is_none());
    });
}

#[test]
fn submit_assigns_monotonic_indices() {
    TestState::default().build_and_execute(|| {
        let i0 = submit_on(TRACK_PASS_OR_FAIL, U256::from(PROPOSER));
        let i1 = submit_on(TRACK_ADJUSTABLE, U256::from(PROPOSER));
        let i2 = submit_on(TRACK_PASS_OR_FAIL, U256::from(PROPOSER_B));
        assert_eq!((i0, i1, i2), (0, 1, 2));
        assert_eq!(ReferendumCount::<Test>::get(), 3);
        assert_eq!(ActiveCount::<Test>::get(), 3);
    });
}

#[test]
fn submit_rejects_invalid_origins_and_tracks() {
    TestState::default().build_and_execute(|| {
        // Bad track id.
        assert_noop!(
            Referenda::submit(
                RuntimeOrigin::signed(U256::from(PROPOSER)),
                99u8,
                Box::new(make_call()),
            ),
            Error::<Test>::BadTrack
        );
        // Root and unsigned both fail; submit takes a signed origin only.
        assert_noop!(
            Referenda::submit(
                RuntimeOrigin::root(),
                TRACK_PASS_OR_FAIL,
                Box::new(make_call())
            ),
            DispatchError::BadOrigin
        );
        // Caller is not in the proposer set.
        assert_noop!(
            Referenda::submit(
                RuntimeOrigin::signed(U256::from(999)),
                TRACK_PASS_OR_FAIL,
                Box::new(make_call()),
            ),
            Error::<Test>::NotProposer
        );
        // Track has no proposer set.
        assert_noop!(
            Referenda::submit(
                RuntimeOrigin::signed(U256::from(PROPOSER)),
                TRACK_NO_PROPOSER_SET,
                Box::new(make_call()),
            ),
            Error::<Test>::TrackNotSubmittable
        );
    });
}

/// A track whose voter set is currently empty would mathematically
/// freeze its tally at zero and drive the referendum to a fixed
/// outcome regardless of merit (auto-enactment on `Adjustable`,
/// expiry on `PassOrFail`). `submit` must refuse rather than create
/// such a referendum.
#[test]
fn submit_rejects_when_voter_set_is_empty() {
    TestState {
        proposers: vec![U256::from(PROPOSER)],
        // Triumvirate is the voter set for tracks 0/1/2; leave it empty
        // so `voter_set.is_empty()` triggers at submit time.
        triumvirate: vec![],
    }
    .build_and_execute(|| {
        assert_noop!(
            Referenda::submit(
                RuntimeOrigin::signed(U256::from(PROPOSER)),
                TRACK_PASS_OR_FAIL,
                Box::new(make_call()),
            ),
            Error::<Test>::EmptyVoterSet
        );
        // No state mutated: index counter unchanged, no referendum stored.
        assert_eq!(ReferendumCount::<Test>::get(), 0);
        assert_eq!(ActiveCount::<Test>::get(), 0);
    });
}

#[test]
fn submit_rejects_call_when_authorize_proposal_returns_false() {
    TestState::default().build_and_execute(|| {
        set_authorize_proposal(false);
        assert_noop!(
            Referenda::submit(
                RuntimeOrigin::signed(U256::from(PROPOSER)),
                TRACK_PASS_OR_FAIL,
                Box::new(make_call()),
            ),
            Error::<Test>::ProposalNotAuthorized
        );
    });
}

#[test]
fn submit_caps_at_max_queued_and_recycles_after_kill() {
    let max_queued = <Test as Config>::MaxQueued::get();
    let per_proposer = <Test as Config>::MaxActivePerProposer::get();
    let proposer_count = max_queued.div_ceil(per_proposer);
    let proposers: Vec<U256> = (1..=proposer_count).map(U256::from).collect();

    TestState {
        proposers: proposers.clone(),
        ..Default::default()
    }
    .build_and_execute(|| {
        let mut submitted = 0u32;
        'fill: for proposer in &proposers {
            for _ in 0..per_proposer {
                if submitted == max_queued {
                    break 'fill;
                }
                assert_ok!(Referenda::submit(
                    RuntimeOrigin::signed(*proposer),
                    TRACK_PASS_OR_FAIL,
                    Box::new(make_call()),
                ));
                submitted += 1;
            }
        }
        assert_eq!(ActiveCount::<Test>::get(), max_queued);

        let next_proposer = U256::from(proposer_count + 1);
        pallet_multi_collective::Pallet::<Test>::add_member(
            RuntimeOrigin::root(),
            CollectiveId::Proposers,
            next_proposer,
        )
        .unwrap();
        assert_noop!(
            Referenda::submit(
                RuntimeOrigin::signed(next_proposer),
                TRACK_PASS_OR_FAIL,
                Box::new(make_call()),
            ),
            Error::<Test>::QueueFull
        );

        assert_ok!(Referenda::kill(RuntimeOrigin::root(), 5));
        assert_ok!(Referenda::submit(
            RuntimeOrigin::signed(next_proposer),
            TRACK_PASS_OR_FAIL,
            Box::new(make_call()),
        ));
        assert_eq!(ActiveCount::<Test>::get(), max_queued);
    });
}

#[test]
fn submit_caps_at_per_proposer_quota_and_recycles_after_kill() {
    let cap = <Test as Config>::MaxActivePerProposer::get();
    TestState::default().build_and_execute(|| {
        let mut indices = Vec::new();
        for _ in 0..cap {
            indices.push(submit_on(TRACK_PASS_OR_FAIL, U256::from(PROPOSER)));
        }
        assert_eq!(ActivePerProposer::<Test>::get(U256::from(PROPOSER)), cap);

        assert_noop!(
            Referenda::submit(
                RuntimeOrigin::signed(U256::from(PROPOSER)),
                TRACK_PASS_OR_FAIL,
                Box::new(make_call()),
            ),
            Error::<Test>::ProposerQuotaExceeded
        );

        assert_ok!(Referenda::submit(
            RuntimeOrigin::signed(U256::from(PROPOSER_B)),
            TRACK_PASS_OR_FAIL,
            Box::new(make_call()),
        ));

        assert_ok!(Referenda::kill(RuntimeOrigin::root(), indices[0]));
        assert_eq!(
            ActivePerProposer::<Test>::get(U256::from(PROPOSER)),
            cap - 1
        );

        assert_ok!(Referenda::submit(
            RuntimeOrigin::signed(U256::from(PROPOSER)),
            TRACK_PASS_OR_FAIL,
            Box::new(make_call()),
        ));
        assert_eq!(ActivePerProposer::<Test>::get(U256::from(PROPOSER)), cap);
    });
}

#[test]
fn kill_concludes_with_killed_status_and_full_cleanup() {
    TestState::default().build_and_execute(|| {
        let index = submit_on(TRACK_ADJUSTABLE, U256::from(PROPOSER));
        run_to_block(current_block() + 5);
        let killed_at = current_block();

        assert_ok!(Referenda::kill(RuntimeOrigin::root(), index));

        assert!(matches!(status_of(index), ReferendumStatus::Killed(b) if b == killed_at));
        assert_concluded(index, 0);
        assert!(Pallet::<Test>::next_task_dispatch_time(index).is_none());
        assert!(has_event(
            |e| matches!(e, Event::Killed { index: i } if *i == index)
        ));
    });
}

#[test]
fn kill_rejects_non_kill_origin_and_unknown_index() {
    TestState::default().build_and_execute(|| {
        let index = submit_on(TRACK_PASS_OR_FAIL, U256::from(PROPOSER));
        assert_noop!(
            Referenda::kill(RuntimeOrigin::signed(U256::from(PROPOSER)), index),
            DispatchError::BadOrigin
        );
        assert_noop!(
            Referenda::kill(RuntimeOrigin::root(), 999),
            Error::<Test>::ReferendumNotFound
        );
    });
}

#[test]
fn kill_rejects_already_finalized_referendum_for_every_terminal_status() {
    // `kill` accepts states that still hold scheduler hooks
    // (`Ongoing`, `Approved`, `FastTracked`); it must reject every other
    // terminal status with `ReferendumFinalized`.
    TestState::default().build_and_execute(|| {
        // Killed.
        let i = submit_on(TRACK_PASS_OR_FAIL, U256::from(PROPOSER));
        assert_ok!(Referenda::kill(RuntimeOrigin::root(), i));
        assert_noop!(
            Referenda::kill(RuntimeOrigin::root(), i),
            Error::<Test>::ReferendumFinalized
        );

        // Enacted (after the wrapper dispatches).
        let i = submit_on(TRACK_PASS_OR_FAIL, U256::from(PROPOSER));
        vote(VOTER_A, i, true);
        vote(VOTER_B, i, true);
        run_to_block(current_block() + 2);
        assert!(matches!(status_of(i), ReferendumStatus::Enacted(_)));
        assert_noop!(
            Referenda::kill(RuntimeOrigin::root(), i),
            Error::<Test>::ReferendumFinalized
        );

        // Rejected.
        let i = submit_on(TRACK_PASS_OR_FAIL, U256::from(PROPOSER));
        vote(VOTER_A, i, false);
        vote(VOTER_B, i, false);
        run_to_block(current_block() + 2);
        assert!(matches!(status_of(i), ReferendumStatus::Rejected(_)));
        assert_noop!(
            Referenda::kill(RuntimeOrigin::root(), i),
            Error::<Test>::ReferendumFinalized
        );

        // Expired.
        let i = submit_on(TRACK_PASS_OR_FAIL, U256::from(PROPOSER));
        run_to_block(current_block() + DECISION_PERIOD + 1);
        assert!(matches!(status_of(i), ReferendumStatus::Expired(_)));
        assert_noop!(
            Referenda::kill(RuntimeOrigin::root(), i),
            Error::<Test>::ReferendumFinalized
        );

        // Cancelled.
        let i = submit_on(TRACK_ADJUSTABLE, U256::from(PROPOSER));
        vote(VOTER_A, i, false);
        vote(VOTER_B, i, false);
        run_to_block(current_block() + 2);
        assert!(matches!(status_of(i), ReferendumStatus::Cancelled(_)));
        assert_noop!(
            Referenda::kill(RuntimeOrigin::root(), i),
            Error::<Test>::ReferendumFinalized
        );

        // Delegated.
        let i = submit_on(TRACK_DELEGATING, U256::from(PROPOSER));
        vote(VOTER_A, i, true);
        vote(VOTER_B, i, true);
        run_to_block(current_block() + 2);
        assert!(matches!(status_of(i), ReferendumStatus::Delegated(_)));
        assert_noop!(
            Referenda::kill(RuntimeOrigin::root(), i),
            Error::<Test>::ReferendumFinalized
        );
    });
}

#[test]
fn kill_succeeds_on_approved_and_releases_wrapper_preimage() {
    assert_kill_drops_wrapper_after(TRACK_PASS_OR_FAIL, &[VOTER_A, VOTER_B], |s| {
        matches!(s, ReferendumStatus::Approved(_))
    });
}

#[test]
fn kill_succeeds_on_fast_tracked_and_releases_wrapper_preimage() {
    assert_kill_drops_wrapper_after(TRACK_ADJUSTABLE, &[VOTER_A, VOTER_B, VOTER_C], |s| {
        matches!(s, ReferendumStatus::FastTracked(_))
    });
}

#[test]
fn advance_referendum_origin_and_index_validation() {
    TestState::default().build_and_execute(|| {
        let index = submit_on(TRACK_PASS_OR_FAIL, U256::from(PROPOSER));
        assert_noop!(
            Referenda::advance_referendum(RuntimeOrigin::signed(U256::from(PROPOSER)), index),
            DispatchError::BadOrigin
        );
        assert_noop!(
            Referenda::advance_referendum(RuntimeOrigin::root(), 999),
            Error::<Test>::ReferendumNotFound
        );
    });
}

#[test]
fn advance_referendum_on_ongoing_runs_the_decision_logic() {
    TestState::default().build_and_execute(|| {
        let index = submit_on(TRACK_PASS_OR_FAIL, U256::from(PROPOSER));
        vote(VOTER_A, index, true);
        vote(VOTER_B, index, true);
        // Manual advance instead of waiting for the alarm.
        assert_ok!(Referenda::advance_referendum(RuntimeOrigin::root(), index));
        assert!(matches!(status_of(index), ReferendumStatus::Approved(_)));
    });
}

#[test]
fn advance_referendum_is_a_noop_for_every_terminal_status() {
    TestState::default().build_and_execute(|| {
        // Killed.
        let i = submit_on(TRACK_PASS_OR_FAIL, U256::from(PROPOSER));
        assert_ok!(Referenda::kill(RuntimeOrigin::root(), i));
        let snapshot = status_of(i);
        assert_ok!(Referenda::advance_referendum(RuntimeOrigin::root(), i));
        assert_eq!(status_of(i), snapshot);

        // Rejected.
        let i = submit_on(TRACK_PASS_OR_FAIL, U256::from(PROPOSER));
        vote(VOTER_A, i, false);
        vote(VOTER_B, i, false);
        run_to_block(current_block() + 2);
        let snapshot = status_of(i);
        assert_ok!(Referenda::advance_referendum(RuntimeOrigin::root(), i));
        assert_eq!(status_of(i), snapshot);

        // Enacted.
        let i = submit_on(TRACK_ADJUSTABLE, U256::from(PROPOSER));
        run_to_block(current_block() + INITIAL_DELAY + 5);
        let snapshot = status_of(i);
        assert_ok!(Referenda::advance_referendum(RuntimeOrigin::root(), i));
        assert_eq!(status_of(i), snapshot);

        // Delegated.
        let i = submit_on(TRACK_DELEGATING, U256::from(PROPOSER));
        vote(VOTER_A, i, true);
        vote(VOTER_B, i, true);
        run_to_block(current_block() + 2);
        let snapshot = status_of(i);
        assert_ok!(Referenda::advance_referendum(RuntimeOrigin::root(), i));
        assert_eq!(status_of(i), snapshot);

        // Expired.
        let i = submit_on(TRACK_PASS_OR_FAIL, U256::from(PROPOSER));
        run_to_block(current_block() + DECISION_PERIOD + 1);
        assert!(matches!(status_of(i), ReferendumStatus::Expired(_)));
        let snapshot = status_of(i);
        assert_ok!(Referenda::advance_referendum(RuntimeOrigin::root(), i));
        assert_eq!(status_of(i), snapshot);

        // Cancelled.
        let i = submit_on(TRACK_ADJUSTABLE, U256::from(PROPOSER));
        vote(VOTER_A, i, false);
        vote(VOTER_B, i, false);
        run_to_block(current_block() + 2);
        assert!(matches!(status_of(i), ReferendumStatus::Cancelled(_)));
        let snapshot = status_of(i);
        assert_ok!(Referenda::advance_referendum(RuntimeOrigin::root(), i));
        assert_eq!(status_of(i), snapshot);

        // Approved (transient one-block window before the wrapper dispatches).
        let i = submit_on(TRACK_PASS_OR_FAIL, U256::from(PROPOSER));
        vote(VOTER_A, i, true);
        vote(VOTER_B, i, true);
        run_to_block(current_block() + 1);
        assert!(matches!(status_of(i), ReferendumStatus::Approved(_)));
        let snapshot = status_of(i);
        assert_ok!(Referenda::advance_referendum(RuntimeOrigin::root(), i));
        assert_eq!(status_of(i), snapshot);

        // FastTracked (transient one-block window before the wrapper dispatches).
        let i = submit_on(TRACK_ADJUSTABLE, U256::from(PROPOSER));
        vote(VOTER_A, i, true);
        vote(VOTER_B, i, true);
        vote(VOTER_C, i, true);
        run_to_block(current_block() + 1);
        assert!(matches!(status_of(i), ReferendumStatus::FastTracked(_)));
        let snapshot = status_of(i);
        assert_ok!(Referenda::advance_referendum(RuntimeOrigin::root(), i));
        assert_eq!(status_of(i), snapshot);
    });
}

#[test]
fn enact_rejects_non_root_origin() {
    TestState::default().build_and_execute(|| {
        assert_noop!(
            Referenda::enact(
                RuntimeOrigin::signed(U256::from(PROPOSER)),
                0,
                Box::new(make_call())
            ),
            DispatchError::BadOrigin
        );
    });
}

#[test]
fn enact_noops_on_terminal_status_so_stale_task_cannot_dispatch() {
    TestState::default().build_and_execute(|| {
        let index = submit_on(TRACK_ADJUSTABLE, U256::from(PROPOSER));

        assert_ok!(Referenda::kill(RuntimeOrigin::root(), index));
        assert!(matches!(status_of(index), ReferendumStatus::Killed(_)));

        assert_ok!(Referenda::enact(
            RuntimeOrigin::root(),
            index,
            Box::new(make_call())
        ));
        assert!(matches!(status_of(index), ReferendumStatus::Killed(_)));
    });
}

#[test]
fn enact_noops_on_unknown_index() {
    TestState::default().build_and_execute(|| {
        assert_ok!(Referenda::enact(
            RuntimeOrigin::root(),
            999,
            Box::new(make_call())
        ));
    });
}

#[test]
fn enact_event_carries_inner_dispatch_result() {
    TestState::default().build_and_execute(|| {
        let ok_index = submit_on(TRACK_PASS_OR_FAIL, U256::from(PROPOSER));
        assert_ok!(Referenda::enact(
            RuntimeOrigin::root(),
            ok_index,
            Box::new(make_call())
        ));
        assert!(has_event(|e| matches!(
            e,
            Event::Enacted { index: i, error: None, .. } if *i == ok_index
        )));

        // pallet_balances::transfer_keep_alive requires a signed origin;
        // dispatching it with Root yields BadOrigin.
        let bad_index = submit_on(TRACK_PASS_OR_FAIL, U256::from(PROPOSER));
        let bad_call = RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
            dest: U256::from(VOTER_A),
            value: 1,
        });
        assert_ok!(Referenda::enact(
            RuntimeOrigin::root(),
            bad_index,
            Box::new(bad_call)
        ));
        assert!(has_event(|e| matches!(
            e,
            Event::Enacted { index: i, error: Some(_), .. } if *i == bad_index
        )));
    });
}

#[test]
fn pass_or_fail_below_threshold_stays_ongoing() {
    TestState::default().build_and_execute(|| {
        let aye_only = submit_on(TRACK_PASS_OR_FAIL, U256::from(PROPOSER));
        vote(VOTER_A, aye_only, true);
        run_to_block(current_block() + 2);
        assert!(Referenda::is_ongoing(aye_only));

        let nay_only = submit_on(TRACK_PASS_OR_FAIL, U256::from(PROPOSER));
        vote(VOTER_A, nay_only, false);
        run_to_block(current_block() + 2);
        assert!(Referenda::is_ongoing(nay_only));
    });
}

#[test]
fn pass_or_fail_approves_at_threshold_and_reaches_enacted() {
    TestState::default().build_and_execute(|| {
        let index = submit_on(TRACK_PASS_OR_FAIL, U256::from(PROPOSER));

        vote(VOTER_A, index, true);
        vote(VOTER_B, index, true);
        run_to_block(current_block() + 1);

        assert!(matches!(status_of(index), ReferendumStatus::Approved(_)));
        assert!(has_event(
            |e| matches!(e, Event::Approved { index: i } if *i == index)
        ));

        run_to_block(current_block() + 1);
        assert!(matches!(status_of(index), ReferendumStatus::Enacted(_)));
        assert!(has_event(|e| matches!(
            e,
            Event::Enacted { index: i, error: None, .. } if *i == index
        )));
    });
}

#[test]
fn pass_or_fail_rejects_at_threshold_with_full_cleanup() {
    TestState::default().build_and_execute(|| {
        let index = submit_on(TRACK_PASS_OR_FAIL, U256::from(PROPOSER));

        vote(VOTER_A, index, false);
        vote(VOTER_B, index, false);
        run_to_block(current_block() + 2);

        assert!(matches!(status_of(index), ReferendumStatus::Rejected(_)));
        assert_concluded(index, 0);
        assert!(has_event(
            |e| matches!(e, Event::Rejected { index: i } if *i == index)
        ));
    });
}

#[test]
fn pass_or_fail_expires_at_deadline_with_full_cleanup() {
    TestState::default().build_and_execute(|| {
        let index = submit_on(TRACK_PASS_OR_FAIL, U256::from(PROPOSER));
        let submitted = current_block();

        run_to_block(submitted + DECISION_PERIOD - 1);
        assert!(Referenda::is_ongoing(index));

        run_to_block(submitted + DECISION_PERIOD);
        assert!(matches!(status_of(index), ReferendumStatus::Expired(_)));
        assert_concluded(index, 0);
        assert!(has_event(
            |e| matches!(e, Event::Expired { index: i } if *i == index)
        ));
    });
}

#[test]
fn pass_or_fail_non_decisive_vote_does_not_prematurely_expire() {
    TestState::default().build_and_execute(|| {
        let index = submit_on(TRACK_PASS_OR_FAIL, U256::from(PROPOSER));
        let submitted = current_block();

        vote(VOTER_A, index, true);
        run_to_block(current_block() + 5);

        assert!(Referenda::is_ongoing(index));
        assert_eq!(
            scheduler_alarm_block(index),
            Some(submitted + DECISION_PERIOD),
            "deadline alarm should be restored"
        );

        // Without further votes, the deadline alarm still fires the expiry.
        run_to_block(submitted + DECISION_PERIOD + 1);
        assert!(matches!(status_of(index), ReferendumStatus::Expired(_)));
    });
}

#[test]
fn pass_or_fail_decisive_vote_at_last_block_of_deadline_approves() {
    TestState::default().build_and_execute(|| {
        let index = submit_on(TRACK_PASS_OR_FAIL, U256::from(PROPOSER));
        let submitted = current_block();

        run_to_block(submitted + DECISION_PERIOD - 1);
        vote(VOTER_A, index, true);
        vote(VOTER_B, index, true);
        run_to_block(current_block() + 1);

        assert!(matches!(status_of(index), ReferendumStatus::Approved(_)));
    });
}

#[test]
fn pass_or_fail_vote_change_can_flip_outcome_before_alarm_fires() {
    TestState::default().build_and_execute(|| {
        let index = submit_on(TRACK_PASS_OR_FAIL, U256::from(PROPOSER));

        vote(VOTER_A, index, true);
        vote(VOTER_B, index, true);
        // Voter B changes mind before the alarm fires; tally drops below
        // approval threshold.
        vote(VOTER_B, index, false);

        run_to_block(current_block() + 2);
        assert!(Referenda::is_ongoing(index));
    });
}

#[test]
fn do_approve_fails_closed_when_review_target_is_unusable() {
    TestState::default().build_and_execute(|| {
        let parent = submit_on(TRACK_DELEGATING, U256::from(PROPOSER));
        let submitted = current_block();

        let _guard = HideReviewTrackGuard::new(true);

        vote(VOTER_A, parent, true);
        vote(VOTER_B, parent, true);
        run_to_block(current_block() + 2);

        assert!(matches!(status_of(parent), ReferendumStatus::Ongoing(_)));
        assert!(ReferendumStatusFor::<Test>::get(parent + 1).is_none());

        let events = referenda_events();
        assert!(!events.iter().any(|e| matches!(e, Event::Approved { .. })));
        assert!(!events.iter().any(|e| matches!(e, Event::Delegated { .. })));
        assert!(!events.iter().any(|e| matches!(e, Event::Enacted { .. })));
        assert!(events.iter().any(|e| matches!(
            e,
            Event::ReviewSchedulingFailed { index, track }
                if *index == parent && *track == TRACK_ADJUSTABLE
        )));

        let deadline = submitted + DECISION_PERIOD;
        assert_eq!(scheduler_alarm_block(parent), Some(deadline));
    });
}

#[test]
fn do_approve_review_failure_expires_at_deadline() {
    TestState::default().build_and_execute(|| {
        let parent = submit_on(TRACK_DELEGATING, U256::from(PROPOSER));

        let _guard = HideReviewTrackGuard::new(true);

        vote(VOTER_A, parent, true);
        vote(VOTER_B, parent, true);
        run_to_block(current_block() + 2);
        assert!(matches!(status_of(parent), ReferendumStatus::Ongoing(_)));

        run_to_block(current_block() + DECISION_PERIOD + 1);

        assert!(matches!(status_of(parent), ReferendumStatus::Expired(_)));
        assert_concluded(parent, 0);
    });
}

#[test]
fn do_approve_fails_closed_when_review_voter_set_is_empty() {
    TestState::default().build_and_execute(|| {
        let parent = submit_on(TRACK_DELEGATING, U256::from(PROPOSER));

        let _guard = EmptyReviewVoterSetGuard::new(true);

        vote(VOTER_A, parent, true);
        vote(VOTER_B, parent, true);
        run_to_block(current_block() + 2);

        assert!(matches!(status_of(parent), ReferendumStatus::Ongoing(_)));
        assert!(ReferendumStatusFor::<Test>::get(parent + 1).is_none());

        let events = referenda_events();
        assert!(events.iter().any(|e| matches!(
            e,
            Event::ReviewSchedulingFailed { index, track }
                if *index == parent && *track == TRACK_ADJUSTABLE
        )));
    });
}

#[test]
fn do_approve_review_recovers_when_track_is_restored() {
    TestState::default().build_and_execute(|| {
        let parent = submit_on(TRACK_DELEGATING, U256::from(PROPOSER));

        {
            let _guard = HideReviewTrackGuard::new(true);
            vote(VOTER_A, parent, true);
            vote(VOTER_B, parent, true);
            run_to_block(current_block() + 2);
            assert!(matches!(status_of(parent), ReferendumStatus::Ongoing(_)));
        }

        assert_ok!(Referenda::advance_referendum(RuntimeOrigin::root(), parent));

        let child = parent + 1;
        assert!(matches!(status_of(parent), ReferendumStatus::Delegated(_)));
        assert!(matches!(status_of(child), ReferendumStatus::Ongoing(_)));
    });
}

#[test]
fn do_approve_fails_closed_when_schedule_enactment_fails() {
    use frame_support::traits::{
        StorePreimage,
        schedule::{DispatchTime, v3::Named},
    };

    TestState::default().build_and_execute(|| {
        let index = submit_on(TRACK_PASS_OR_FAIL, U256::from(PROPOSER));
        let submitted = current_block();

        let dummy = <Preimage as StorePreimage>::bound::<RuntimeCall>(make_call()).unwrap();
        <Scheduler as Named<u64, RuntimeCall, OriginCaller>>::schedule_named(
            task_name(index),
            DispatchTime::At(submitted + 1000),
            None,
            0,
            frame_system::RawOrigin::Root.into(),
            dummy,
        )
        .unwrap();

        vote(VOTER_A, index, true);
        vote(VOTER_B, index, true);
        run_to_block(current_block() + 1);

        assert!(matches!(status_of(index), ReferendumStatus::Ongoing(_)));
        let events = referenda_events();
        assert!(!events.iter().any(|e| matches!(e, Event::Approved { .. })));
        assert!(!events.iter().any(|e| matches!(e, Event::Enacted { .. })));
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::SchedulerOperationFailed { index: i } if *i == index))
        );
        assert_eq!(
            scheduler_alarm_block(index),
            Some(submitted + DECISION_PERIOD)
        );
    });
}

#[test]
fn adjustable_without_votes_keeps_initial_delay() {
    TestState::default().build_and_execute(|| {
        let index = submit_on(TRACK_ADJUSTABLE, U256::from(PROPOSER));
        let submitted = current_block();
        assert_eq!(
            Pallet::<Test>::next_task_dispatch_time(index),
            Some(submitted + INITIAL_DELAY)
        );
    });
}

#[test]
fn adjustable_lapses_to_enacted_when_no_decisive_votes() {
    TestState::default().build_and_execute(|| {
        let index = submit_on(TRACK_ADJUSTABLE, U256::from(PROPOSER));
        let submitted = current_block();

        run_to_block(submitted + INITIAL_DELAY + 5);

        assert!(matches!(status_of(index), ReferendumStatus::Enacted(_)));
        assert_concluded(index, 0);

        let events = referenda_events();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::Enacted { index: i, .. } if *i == index))
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Event::Approved { .. } | Event::FastTracked { .. })),
            "lapse should not emit Approved or FastTracked"
        );
    });
}

#[test]
fn adjustable_progresses_through_approval_curve_into_fast_track() {
    TestState::default().build_and_execute(|| {
        let index = submit_on(TRACK_ADJUSTABLE, U256::from(PROPOSER));
        let start = current_block();
        let initial_target = start + INITIAL_DELAY;

        vote(VOTER_A, index, true);
        run_to_block(start + 1);
        let after_one = Pallet::<Test>::next_task_dispatch_time(index).unwrap();
        assert!(after_one < initial_target);

        vote(VOTER_B, index, true);
        run_to_block(start + 2);
        let after_two = Pallet::<Test>::next_task_dispatch_time(index).unwrap();
        assert!(
            after_two < after_one,
            "each successive aye should pull the target strictly earlier"
        );

        vote(VOTER_C, index, true);
        run_to_block(start + 5);
        assert!(matches!(status_of(index), ReferendumStatus::Enacted(_)));
        assert!(has_event(
            |e| matches!(e, Event::FastTracked { index: i } if *i == index)
        ));
    });
}

#[test]
fn adjustable_progresses_through_rejection_curve_into_cancel() {
    TestState::default().build_and_execute(|| {
        let index = submit_on(TRACK_ADJUSTABLE, U256::from(PROPOSER));
        let start = current_block();
        let initial_target = start + INITIAL_DELAY;

        vote(VOTER_A, index, false);
        run_to_block(start + 1);
        let after_one = Pallet::<Test>::next_task_dispatch_time(index).unwrap();
        assert!(after_one > initial_target);

        vote(VOTER_B, index, false);
        run_to_block(start + 2);
        assert!(matches!(status_of(index), ReferendumStatus::Cancelled(_)));
        assert!(has_event(
            |e| matches!(e, Event::Cancelled { index: i } if *i == index)
        ));
    });
}

#[test]
fn adjustable_balanced_votes_keep_initial_delay() {
    TestState::default().build_and_execute(|| {
        let index = submit_on(TRACK_ADJUSTABLE, U256::from(PROPOSER));
        let start = current_block();

        vote(VOTER_A, index, true);
        vote(VOTER_B, index, false);
        run_to_block(start + 1);

        assert_eq!(
            Pallet::<Test>::next_task_dispatch_time(index),
            Some(start + INITIAL_DELAY),
            "net-zero votes should leave the target at initial_delay"
        );
    });
}

#[test]
fn adjustable_repeated_flips_return_target_to_same_value() {
    TestState::default().build_and_execute(|| {
        let index = submit_on(TRACK_ADJUSTABLE, U256::from(PROPOSER));
        let start = current_block();
        let initial_target = start + INITIAL_DELAY;

        vote(VOTER_A, index, false);
        run_to_block(start + 1);
        let nay_1 = Pallet::<Test>::next_task_dispatch_time(index).unwrap();
        assert!(nay_1 > initial_target);

        vote(VOTER_A, index, true);
        run_to_block(start + 2);
        let aye_1 = Pallet::<Test>::next_task_dispatch_time(index).unwrap();
        assert!(aye_1 < initial_target);

        vote(VOTER_A, index, false);
        run_to_block(start + 3);
        let nay_2 = Pallet::<Test>::next_task_dispatch_time(index).unwrap();
        assert_eq!(
            nay_1, nay_2,
            "flipping back to the same tally should land at the same target"
        );

        vote(VOTER_A, index, true);
        run_to_block(start + 4);
        let aye_2 = Pallet::<Test>::next_task_dispatch_time(index).unwrap();
        assert_eq!(aye_1, aye_2);
    });
}

#[test]
fn adjustable_target_is_stable_across_elapsed_blocks() {
    // The interpolation is anchored at `submitted`, so sitting through
    // blocks without new votes does not drift the target forward.
    TestState::default().build_and_execute(|| {
        let index = submit_on(TRACK_ADJUSTABLE, U256::from(PROPOSER));

        vote(VOTER_A, index, true);
        run_to_block(current_block() + 2);
        let target_after_vote = Pallet::<Test>::next_task_dispatch_time(index).unwrap();

        run_to_block(current_block() + 10);
        let target_later = Pallet::<Test>::next_task_dispatch_time(index).unwrap();
        assert_eq!(target_after_vote, target_later);
    });
}

#[test]
fn adjustable_late_vote_when_target_is_in_the_past_fast_tracks() {
    TestState::default().build_and_execute(|| {
        let index = submit_on(TRACK_ADJUSTABLE, U256::from(PROPOSER));
        let submitted = current_block();

        // Run forward past where the partial-approval target would land.
        run_to_block(submitted + INITIAL_DELAY / 2 + 10);

        vote(VOTER_A, index, true);
        run_to_block(current_block() + 5);

        assert!(matches!(status_of(index), ReferendumStatus::Enacted(_)));
        assert!(has_event(
            |e| matches!(e, Event::FastTracked { index: i } if *i == index)
        ));
    });
}

#[test]
fn adjustable_delayed_then_accelerated_fast_tracks_via_past_target() {
    TestState::default().build_and_execute(|| {
        let index = submit_on(TRACK_ADJUSTABLE, U256::from(PROPOSER));
        let start = current_block();
        let initial_target = start + INITIAL_DELAY;

        // Push the enactment task past `initial_target` with a nay.
        vote(VOTER_A, index, false);
        run_to_block(start + 1);
        let extended = Pallet::<Test>::next_task_dispatch_time(index).unwrap();
        assert!(extended > initial_target);

        // Cross the original deadline without firing (target is now extended).
        run_to_block(initial_target + 10);

        // Counter-vote pulls the recomputed target back to `initial_target`,
        // which is already in the past; `do_adjust_delay` flips to fast-track.
        vote(VOTER_B, index, true);
        run_to_block(initial_target + 15);

        assert!(matches!(status_of(index), ReferendumStatus::Enacted(_)));
        assert!(has_event(
            |e| matches!(e, Event::FastTracked { index: i } if *i == index)
        ));
    });
}

#[test]
fn adjustable_fast_tracks_at_threshold_and_reaches_enacted() {
    TestState::default().build_and_execute(|| {
        let index = submit_on(TRACK_ADJUSTABLE, U256::from(PROPOSER));

        vote(VOTER_A, index, true);
        vote(VOTER_B, index, true);
        vote(VOTER_C, index, true);
        run_to_block(current_block() + 5);

        assert!(matches!(status_of(index), ReferendumStatus::Enacted(_)));
        let events = referenda_events();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::FastTracked { index: i } if *i == index))
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::Enacted { index: i, .. } if *i == index))
        );
    });
}

#[test]
fn adjustable_cancels_at_threshold_and_cleans_up_task() {
    TestState::default().build_and_execute(|| {
        let index = submit_on(TRACK_ADJUSTABLE, U256::from(PROPOSER));

        vote(VOTER_A, index, false);
        vote(VOTER_B, index, false);
        run_to_block(current_block() + 2);

        assert!(matches!(status_of(index), ReferendumStatus::Cancelled(_)));
        assert_concluded(index, 0);
        assert!(Pallet::<Test>::next_task_dispatch_time(index).is_none());
        assert!(has_event(
            |e| matches!(e, Event::Cancelled { index: i } if *i == index)
        ));
    });
}

#[test]
fn adjustable_non_decisive_vote_still_reaches_enacted_via_enact_wrapper() {
    TestState::default().build_and_execute(|| {
        let index = submit_on(TRACK_ADJUSTABLE, U256::from(PROPOSER));
        let submitted = current_block();

        vote(VOTER_A, index, true);
        run_to_block(current_block() + 3);
        assert!(Referenda::is_ongoing(index));

        run_to_block(submitted + INITIAL_DELAY + 1);
        assert!(matches!(status_of(index), ReferendumStatus::Enacted(_)));
    });
}

#[test]
fn do_fast_track_fails_closed_when_reschedule_fails() {
    use frame_support::traits::schedule::v3::Named;

    TestState::default().build_and_execute(|| {
        let index = submit_on(TRACK_ADJUSTABLE, U256::from(PROPOSER));

        // Drop the wrapper task so reschedule_named fails with NotFound.
        assert!(
            <Scheduler as Named<u64, RuntimeCall, OriginCaller>>::cancel_named(task_name(index))
                .is_ok()
        );

        Pallet::<Test>::do_fast_track(index);

        assert!(matches!(status_of(index), ReferendumStatus::Ongoing(_)));
        let events = referenda_events();
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Event::FastTracked { .. }))
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::SchedulerOperationFailed { index: i } if *i == index))
        );
    });
}

#[test]
fn delegation_creates_child_review_and_keeps_active_count_net_zero() {
    TestState::default().build_and_execute(|| {
        let parent = submit_on(TRACK_DELEGATING, U256::from(PROPOSER));
        assert_eq!(ActiveCount::<Test>::get(), 1);

        vote(VOTER_A, parent, true);
        vote(VOTER_B, parent, true);
        run_to_block(current_block() + 2);

        let child = parent + 1;

        assert!(matches!(status_of(parent), ReferendumStatus::Delegated(_)));
        match status_of(child) {
            ReferendumStatus::Ongoing(info) => {
                assert_eq!(info.track, TRACK_ADJUSTABLE);
                assert!(matches!(info.proposal, Proposal::Review));
                assert_eq!(info.proposer, U256::from(PROPOSER));
            }
            _ => panic!("child should be Ongoing"),
        }

        // ActiveCount: parent -1, child +1, net unchanged.
        assert_eq!(ActiveCount::<Test>::get(), 1);

        let events = referenda_events();
        assert!(events.iter().any(|e| matches!(
            e,
            Event::Delegated { index, review, track }
                if *index == parent && *review == child && *track == TRACK_ADJUSTABLE
        )));
        // No Submitted for the child, no Approved for the parent.
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, Event::Submitted { .. }))
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, Event::Approved { .. }))
                .count(),
            0
        );
    });
}

#[test]
fn delegated_parent_is_terminal_and_child_progresses_independently() {
    TestState::default().build_and_execute(|| {
        let parent = submit_on(TRACK_DELEGATING, U256::from(PROPOSER));
        vote(VOTER_A, parent, true);
        vote(VOTER_B, parent, true);
        run_to_block(current_block() + 2);
        let child = parent + 1;

        // Manual advance does not promote Delegated.
        let snapshot = status_of(parent);
        assert_ok!(Referenda::advance_referendum(RuntimeOrigin::root(), parent));
        assert_eq!(status_of(parent), snapshot);

        // Child reaches Enacted via natural execution. Parent unchanged.
        run_to_block(current_block() + INITIAL_DELAY + 5);
        assert!(matches!(status_of(child), ReferendumStatus::Enacted(_)));
        assert!(matches!(status_of(parent), ReferendumStatus::Delegated(_)));
    });
}

#[test]
fn killing_child_does_not_change_parent_delegated_status() {
    TestState::default().build_and_execute(|| {
        let parent = submit_on(TRACK_DELEGATING, U256::from(PROPOSER));
        vote(VOTER_A, parent, true);
        vote(VOTER_B, parent, true);
        run_to_block(current_block() + 2);
        let child = parent + 1;

        assert_ok!(Referenda::kill(RuntimeOrigin::root(), child));
        assert!(matches!(status_of(parent), ReferendumStatus::Delegated(_)));
        assert!(matches!(status_of(child), ReferendumStatus::Killed(_)));
    });
}

#[test]
fn schedule_for_review_returns_none_for_invalid_targets() {
    TestState::default().build_and_execute(|| {
        assert!(
            Pallet::<Test>::schedule_for_review(Box::new(make_call()), U256::from(PROPOSER), 99u8,)
                .is_none()
        );

        assert!(
            Pallet::<Test>::schedule_for_review(
                Box::new(make_call()),
                U256::from(PROPOSER),
                TRACK_PASS_OR_FAIL,
            )
            .is_none()
        );

        let _guard = EmptyReviewVoterSetGuard::new(true);
        assert!(
            Pallet::<Test>::schedule_for_review(
                Box::new(make_call()),
                U256::from(PROPOSER),
                TRACK_ADJUSTABLE,
            )
            .is_none()
        );
    });
}

#[test]
fn schedule_for_review_increments_per_proposer_even_above_cap() {
    let cap = <Test as Config>::MaxActivePerProposer::get();
    TestState::default().build_and_execute(|| {
        for _ in 0..cap {
            submit_on(TRACK_PASS_OR_FAIL, U256::from(PROPOSER));
        }
        assert_eq!(ActivePerProposer::<Test>::get(U256::from(PROPOSER)), cap);

        let child = Pallet::<Test>::schedule_for_review(
            Box::new(make_call()),
            U256::from(PROPOSER),
            TRACK_ADJUSTABLE,
        )
        .expect("schedule_for_review must succeed");
        assert!(matches!(status_of(child), ReferendumStatus::Ongoing(_)));
        assert_eq!(
            ActivePerProposer::<Test>::get(U256::from(PROPOSER)),
            cap + 1
        );
    });
}

#[test]
fn polls_returns_some_for_ongoing_and_none_for_every_terminal_status() {
    TestState::default().build_and_execute(|| {
        // Ongoing: the trait returns Some.
        let ongoing = submit_on(TRACK_PASS_OR_FAIL, U256::from(PROPOSER));
        assert!(Referenda::is_ongoing(ongoing));
        assert_eq!(
            Referenda::voting_scheme_of(ongoing),
            Some(VotingScheme::Signed)
        );
        assert!(Referenda::voter_set_of(ongoing).is_some());

        // Helper closures that drive a fresh referendum to each terminal state.
        let killed = drive_to_status(
            || submit_on(TRACK_PASS_OR_FAIL, U256::from(PROPOSER)),
            |i| {
                assert_ok!(Referenda::kill(RuntimeOrigin::root(), i));
            },
        );

        let approved_or_enacted = drive_to_status(
            || submit_on(TRACK_PASS_OR_FAIL, U256::from(PROPOSER)),
            |i| {
                vote(VOTER_A, i, true);
                vote(VOTER_B, i, true);
                drive_to_terminal(i, 50);
            },
        );

        let rejected = drive_to_status(
            || submit_on(TRACK_PASS_OR_FAIL, U256::from(PROPOSER)),
            |i| {
                vote(VOTER_A, i, false);
                vote(VOTER_B, i, false);
                drive_to_terminal(i, 50);
            },
        );

        let expired = drive_to_status(
            || submit_on(TRACK_PASS_OR_FAIL, U256::from(PROPOSER)),
            |i| {
                run_to_block(current_block() + DECISION_PERIOD + 1);
                let _ = i;
            },
        );

        let cancelled = drive_to_status(
            || submit_on(TRACK_ADJUSTABLE, U256::from(PROPOSER)),
            |i| {
                vote(VOTER_A, i, false);
                vote(VOTER_B, i, false);
                drive_to_terminal(i, 50);
            },
        );

        let lapsed = drive_to_status(
            || submit_on(TRACK_ADJUSTABLE, U256::from(PROPOSER)),
            |i| {
                run_to_block(current_block() + INITIAL_DELAY + 5);
                let _ = i;
            },
        );

        let delegated = drive_to_status(
            || submit_on(TRACK_DELEGATING, U256::from(PROPOSER)),
            |i| {
                vote(VOTER_A, i, true);
                vote(VOTER_B, i, true);
                run_to_block(current_block() + 2);
            },
        );

        for terminal in [
            killed,
            approved_or_enacted,
            rejected,
            expired,
            cancelled,
            lapsed,
            delegated,
        ] {
            assert!(!Referenda::is_ongoing(terminal));
            assert!(Referenda::voting_scheme_of(terminal).is_none());
            assert!(Referenda::voter_set_of(terminal).is_none());
        }
    });
}

#[test]
fn polls_returns_none_for_unknown_index() {
    TestState::default().build_and_execute(|| {
        assert!(!Referenda::is_ongoing(999));
        assert!(Referenda::voting_scheme_of(999).is_none());
        assert!(Referenda::voter_set_of(999).is_none());
    });
}

#[test]
fn rejected_drops_submit_time_preimage() {
    TestState::default().build_and_execute(|| {
        let call = make_lookup_call();
        let hash = preimage_hash(&call);

        assert_ok!(Referenda::submit(
            RuntimeOrigin::signed(U256::from(PROPOSER)),
            TRACK_PASS_OR_FAIL,
            Box::new(call),
        ));
        let index = ReferendumCount::<Test>::get() - 1;
        assert!(preimage_exists(&hash));

        vote(VOTER_A, index, false);
        vote(VOTER_B, index, false);
        run_to_block(current_block() + 2);

        assert!(matches!(status_of(index), ReferendumStatus::Rejected(_)));
        assert!(!preimage_exists(&hash));
    });
}

#[test]
fn expired_drops_submit_time_preimage() {
    TestState::default().build_and_execute(|| {
        let call = make_lookup_call();
        let hash = preimage_hash(&call);

        assert_ok!(Referenda::submit(
            RuntimeOrigin::signed(U256::from(PROPOSER)),
            TRACK_PASS_OR_FAIL,
            Box::new(call),
        ));
        let index = ReferendumCount::<Test>::get() - 1;
        let submitted = current_block();
        assert!(preimage_exists(&hash));

        run_to_block(submitted + DECISION_PERIOD);
        assert!(matches!(status_of(index), ReferendumStatus::Expired(_)));
        assert!(!preimage_exists(&hash));
    });
}

#[test]
fn killed_drops_submit_time_preimage_when_action_was_pending() {
    TestState::default().build_and_execute(|| {
        let call = make_lookup_call();
        let hash = preimage_hash(&call);

        assert_ok!(Referenda::submit(
            RuntimeOrigin::signed(U256::from(PROPOSER)),
            TRACK_PASS_OR_FAIL,
            Box::new(call),
        ));
        let index = ReferendumCount::<Test>::get() - 1;
        assert!(preimage_exists(&hash));

        assert_ok!(Referenda::kill(RuntimeOrigin::root(), index));
        assert!(matches!(status_of(index), ReferendumStatus::Killed(_)));
        assert!(!preimage_exists(&hash));
    });
}

#[test]
fn approve_then_enact_drops_both_submit_and_wrapper_preimages() {
    TestState::default().build_and_execute(|| {
        let call = make_lookup_call();
        let submit_hash = preimage_hash(&call);

        assert_ok!(Referenda::submit(
            RuntimeOrigin::signed(U256::from(PROPOSER)),
            TRACK_PASS_OR_FAIL,
            Box::new(call.clone()),
        ));
        let index = ReferendumCount::<Test>::get() - 1;
        let wrapper_hash = enact_wrapper_hash(index, call);
        assert!(preimage_exists(&submit_hash));
        assert!(!preimage_exists(&wrapper_hash));

        vote(VOTER_A, index, true);
        vote(VOTER_B, index, true);
        run_to_block(current_block() + 1);
        assert!(matches!(status_of(index), ReferendumStatus::Approved(_)));
        assert!(!preimage_exists(&submit_hash));
        assert!(preimage_exists(&wrapper_hash));

        run_to_block(current_block() + 1);
        assert!(matches!(status_of(index), ReferendumStatus::Enacted(_)));
        assert!(!preimage_exists(&wrapper_hash));
    });
}

#[test]
fn adjustable_cancel_drops_wrapper_preimage() {
    TestState::default().build_and_execute(|| {
        let call = make_lookup_call();
        let submit_hash = preimage_hash(&call);

        assert_ok!(Referenda::submit(
            RuntimeOrigin::signed(U256::from(PROPOSER)),
            TRACK_ADJUSTABLE,
            Box::new(call.clone()),
        ));
        let index = ReferendumCount::<Test>::get() - 1;
        let wrapper_hash = enact_wrapper_hash(index, call);
        assert!(!preimage_exists(&submit_hash));
        assert!(preimage_exists(&wrapper_hash));

        vote(VOTER_A, index, false);
        vote(VOTER_B, index, false);
        vote(VOTER_C, index, false);
        run_to_block(current_block() + 1);
        assert!(matches!(status_of(index), ReferendumStatus::Cancelled(_)));
        assert!(!preimage_exists(&wrapper_hash));
    });
}

#[test]
fn approve_then_enact_only_decrements_active_count_once() {
    TestState::default().build_and_execute(|| {
        let index = submit_on(TRACK_PASS_OR_FAIL, U256::from(PROPOSER));
        assert_eq!(ActiveCount::<Test>::get(), 1);
        assert_eq!(ActivePerProposer::<Test>::get(U256::from(PROPOSER)), 1);

        vote(VOTER_A, index, true);
        vote(VOTER_B, index, true);
        run_to_block(current_block() + 1);
        assert!(matches!(status_of(index), ReferendumStatus::Approved(_)));
        assert_eq!(ActiveCount::<Test>::get(), 0);
        assert_eq!(ActivePerProposer::<Test>::get(U256::from(PROPOSER)), 0);

        run_to_block(current_block() + 1);
        assert!(matches!(status_of(index), ReferendumStatus::Enacted(_)));
        assert_eq!(ActiveCount::<Test>::get(), 0);
        assert_eq!(ActivePerProposer::<Test>::get(U256::from(PROPOSER)), 0);
    });
}

#[test]
fn fast_track_then_enact_only_decrements_active_count_once() {
    TestState::default().build_and_execute(|| {
        let index = submit_on(TRACK_ADJUSTABLE, U256::from(PROPOSER));
        assert_eq!(ActiveCount::<Test>::get(), 1);
        assert_eq!(ActivePerProposer::<Test>::get(U256::from(PROPOSER)), 1);

        vote(VOTER_A, index, true);
        vote(VOTER_B, index, true);
        vote(VOTER_C, index, true);
        run_to_block(current_block() + 1);
        assert!(matches!(status_of(index), ReferendumStatus::FastTracked(_)));
        assert_eq!(ActiveCount::<Test>::get(), 0);
        assert_eq!(ActivePerProposer::<Test>::get(U256::from(PROPOSER)), 0);

        run_to_block(current_block() + 1);
        assert!(matches!(status_of(index), ReferendumStatus::Enacted(_)));
        assert_eq!(ActiveCount::<Test>::get(), 0);
        assert_eq!(ActivePerProposer::<Test>::get(U256::from(PROPOSER)), 0);
    });
}

#[test]
fn delegated_handoff_keeps_proposer_active_count_at_one() {
    TestState::default().build_and_execute(|| {
        let parent = submit_on(TRACK_DELEGATING, U256::from(PROPOSER));
        assert_eq!(ActivePerProposer::<Test>::get(U256::from(PROPOSER)), 1);

        vote(VOTER_A, parent, true);
        vote(VOTER_B, parent, true);
        run_to_block(current_block() + 2);

        assert!(matches!(status_of(parent), ReferendumStatus::Delegated(_)));
        assert_eq!(ActivePerProposer::<Test>::get(U256::from(PROPOSER)), 1);
    });
}

#[test]
fn submit_snapshots_decision_strategy_into_referendum_info() {
    TestState::default().build_and_execute(|| {
        let index = submit_on(TRACK_PASS_OR_FAIL, U256::from(PROPOSER));
        match status_of(index) {
            ReferendumStatus::Ongoing(info) => {
                assert!(matches!(
                    info.decision_strategy,
                    DecisionStrategy::PassOrFail { .. }
                ));
            }
            _ => panic!("expected Ongoing"),
        }
    });
}

#[test]
fn live_referendum_uses_snapshot_when_track_strategy_changes_at_runtime() {
    TestState::default().build_and_execute(|| {
        let index = submit_on(TRACK_PASS_OR_FAIL, U256::from(PROPOSER));

        let _guard = SwapTrack0ToAdjustableGuard::new(true);

        vote(VOTER_A, index, true);
        vote(VOTER_B, index, true);
        run_to_block(current_block() + 1);

        assert!(matches!(status_of(index), ReferendumStatus::Approved(_)));
    });
}

#[test]
fn alarm_driven_completion_does_not_emit_scheduler_operation_failed() {
    TestState::default().build_and_execute(|| {
        let approved = submit_on(TRACK_PASS_OR_FAIL, U256::from(PROPOSER));
        vote(VOTER_A, approved, true);
        vote(VOTER_B, approved, true);
        run_to_block(current_block() + 1);
        assert!(matches!(status_of(approved), ReferendumStatus::Approved(_)));
        run_to_block(current_block() + 1);
        assert!(matches!(status_of(approved), ReferendumStatus::Enacted(_)));

        let rejected = submit_on(TRACK_PASS_OR_FAIL, U256::from(PROPOSER));
        vote(VOTER_A, rejected, false);
        vote(VOTER_B, rejected, false);
        run_to_block(current_block() + 2);
        assert!(matches!(status_of(rejected), ReferendumStatus::Rejected(_)));

        let expired = submit_on(TRACK_PASS_OR_FAIL, U256::from(PROPOSER));
        let submitted = current_block();
        run_to_block(submitted + DECISION_PERIOD);
        assert!(matches!(status_of(expired), ReferendumStatus::Expired(_)));

        assert!(
            !System::events().iter().any(|record| matches!(
                record.event,
                RuntimeEvent::Referenda(Event::SchedulerOperationFailed { .. })
            )),
            "no SchedulerOperationFailed should fire on routine alarm-driven completions",
        );
    });
}

#[test]
fn set_alarm_replaces_existing_or_arms_fresh() {
    TestState::default().build_and_execute(|| {
        let index = submit_on(TRACK_PASS_OR_FAIL, U256::from(PROPOSER));
        let submitted = current_block();
        assert_eq!(
            scheduler_alarm_block(index),
            Some(submitted + DECISION_PERIOD)
        );

        let events_before_noop = System::events().len();
        assert_ok!(Pallet::<Test>::set_alarm(
            index,
            submitted + DECISION_PERIOD
        ));
        assert_eq!(System::events().len(), events_before_noop);
        assert_eq!(
            scheduler_alarm_block(index),
            Some(submitted + DECISION_PERIOD)
        );

        // Replace.
        assert_ok!(Pallet::<Test>::set_alarm(index, current_block() + 5));
        assert_eq!(scheduler_alarm_block(index), Some(current_block() + 5));

        // Cancel manually, then arm again.
        use frame_support::traits::schedule::v3::Named;
        let _ =
            <Scheduler as Named<u64, RuntimeCall, OriginCaller>>::cancel_named(alarm_name(index));
        assert!(scheduler_alarm_block(index).is_none());

        assert_ok!(Pallet::<Test>::set_alarm(index, current_block() + 10));
        assert_eq!(scheduler_alarm_block(index), Some(current_block() + 10));
    });
}

#[test]
fn parallel_referenda_have_independent_lifecycles() {
    TestState::default().build_and_execute(|| {
        let pf = submit_on(TRACK_PASS_OR_FAIL, U256::from(PROPOSER));
        let adj = submit_on(TRACK_ADJUSTABLE, U256::from(PROPOSER));
        let submitted = current_block();
        assert_eq!(ActiveCount::<Test>::get(), 2);

        // Approve pf; adj must keep its scheduling untouched.
        vote(VOTER_A, pf, true);
        vote(VOTER_B, pf, true);
        run_to_block(current_block() + 5);

        assert!(matches!(status_of(pf), ReferendumStatus::Enacted(_)));
        assert!(Referenda::is_ongoing(adj));
        assert_eq!(
            Pallet::<Test>::next_task_dispatch_time(adj),
            Some(submitted + INITIAL_DELAY)
        );
    });
}

#[test]
fn vote_after_termination_does_not_mutate_referenda_state() {
    TestState::default().build_and_execute(|| {
        let index = submit_on(TRACK_PASS_OR_FAIL, U256::from(PROPOSER));
        assert_ok!(Referenda::kill(RuntimeOrigin::root(), index));

        let active_before = ActiveCount::<Test>::get();
        let status_before = status_of(index);
        let _ = SignedVoting::vote(RuntimeOrigin::signed(U256::from(VOTER_A)), index, true);

        assert_eq!(ActiveCount::<Test>::get(), active_before);
        assert_eq!(status_of(index), status_before);
        assert!(scheduler_alarm_block(index).is_none());
    });
}

#[test]
fn integrity_test_passes_for_valid_track_table() {
    TestState::default().build_and_execute(|| {
        use frame_support::traits::Hooks;
        Pallet::<Test>::integrity_test();
    });
}

#[test]
fn check_integrity_rejects_duplicate_track_ids() {
    assert_check_integrity_err(
        vec![passorfail_track(0), passorfail_track(0)],
        "track ids must be unique",
    );
}

#[test]
fn check_integrity_rejects_review_referencing_unknown_track() {
    let mut t = passorfail_track(0);
    if let DecisionStrategy::PassOrFail {
        ref mut on_approval,
        ..
    } = t.info.decision_strategy
    {
        *on_approval = ApprovalAction::Review { track: 99 };
    }
    assert_check_integrity_err(vec![t], "ApprovalAction::Review references unknown track");
}

#[test]
fn check_integrity_rejects_review_referencing_passorfail_track() {
    let mut t = passorfail_track(0);
    if let DecisionStrategy::PassOrFail {
        ref mut on_approval,
        ..
    } = t.info.decision_strategy
    {
        *on_approval = ApprovalAction::Review { track: 1 };
    }
    let target = passorfail_track(1);
    assert_check_integrity_err(
        vec![t, target],
        "ApprovalAction::Review target track must be Adjustable",
    );
}

#[test]
fn check_integrity_rejects_zero_decision_period() {
    let mut t = passorfail_track(0);
    if let DecisionStrategy::PassOrFail {
        ref mut decision_period,
        ..
    } = t.info.decision_strategy
    {
        *decision_period = 0;
    }
    assert_check_integrity_err(vec![t], "PassOrFail: decision_period must be non-zero");
}

#[test]
fn check_integrity_rejects_zero_approve_threshold() {
    let mut t = passorfail_track(0);
    if let DecisionStrategy::PassOrFail {
        ref mut approve_threshold,
        ..
    } = t.info.decision_strategy
    {
        *approve_threshold = Perbill::zero();
    }
    assert_check_integrity_err(vec![t], "PassOrFail: approve_threshold must be non-zero");
}

#[test]
fn check_integrity_rejects_zero_reject_threshold() {
    let mut t = passorfail_track(0);
    if let DecisionStrategy::PassOrFail {
        ref mut reject_threshold,
        ..
    } = t.info.decision_strategy
    {
        *reject_threshold = Perbill::zero();
    }
    assert_check_integrity_err(vec![t], "PassOrFail: reject_threshold must be non-zero");
}

#[test]
fn check_integrity_rejects_passorfail_thresholds_summing_to_at_most_100_percent() {
    let mut t = passorfail_track(0);
    if let DecisionStrategy::PassOrFail {
        ref mut approve_threshold,
        ref mut reject_threshold,
        ..
    } = t.info.decision_strategy
    {
        *approve_threshold = Perbill::from_percent(50);
        *reject_threshold = Perbill::from_percent(50);
    }
    assert_check_integrity_err(
        vec![t],
        "PassOrFail: approve_threshold + reject_threshold must exceed 100%",
    );
}

#[test]
fn check_integrity_rejects_zero_initial_delay() {
    let mut t = adjustable_track(0);
    if let DecisionStrategy::Adjustable {
        ref mut initial_delay,
        ..
    } = t.info.decision_strategy
    {
        *initial_delay = 0;
    }
    assert_check_integrity_err(vec![t], "Adjustable: initial_delay must be non-zero");
}

#[test]
fn check_integrity_rejects_zero_fast_track_threshold() {
    let mut t = adjustable_track(0);
    if let DecisionStrategy::Adjustable {
        ref mut fast_track_threshold,
        ..
    } = t.info.decision_strategy
    {
        *fast_track_threshold = Perbill::zero();
    }
    assert_check_integrity_err(vec![t], "Adjustable: fast_track_threshold must be non-zero");
}

#[test]
fn check_integrity_rejects_zero_cancel_threshold() {
    let mut t = adjustable_track(0);
    if let DecisionStrategy::Adjustable {
        ref mut cancel_threshold,
        ..
    } = t.info.decision_strategy
    {
        *cancel_threshold = Perbill::zero();
    }
    assert_check_integrity_err(vec![t], "Adjustable: cancel_threshold must be non-zero");
}

#[test]
fn check_integrity_rejects_max_delay_below_initial_delay() {
    let mut t = adjustable_track(0);
    if let DecisionStrategy::Adjustable {
        ref mut max_delay, ..
    } = t.info.decision_strategy
    {
        *max_delay = 50;
    }
    assert_check_integrity_err(vec![t], "Adjustable: max_delay must be >= initial_delay");
}

#[test]
fn check_integrity_rejects_adjustable_thresholds_summing_to_at_most_100_percent() {
    let mut t = adjustable_track(0);
    if let DecisionStrategy::Adjustable {
        ref mut fast_track_threshold,
        ref mut cancel_threshold,
        ..
    } = t.info.decision_strategy
    {
        *fast_track_threshold = Perbill::from_percent(50);
        *cancel_threshold = Perbill::from_percent(50);
    }
    assert_check_integrity_err(
        vec![t],
        "Adjustable: fast_track_threshold + cancel_threshold must exceed 100%",
    );
}

#[test]
fn try_state_passes_with_populated_voter_sets() {
    TestState::default().build_and_execute(|| {
        assert!(Pallet::<Test>::do_try_state().is_ok());
    });
}

#[test]
fn try_state_allows_uninitialized_collectives() {
    TestState {
        proposers: vec![],
        triumvirate: vec![],
    }
    .build_and_execute(|| {
        assert!(Pallet::<Test>::do_try_state().is_ok());
    });
}

#[test]
fn try_state_fails_when_a_track_has_empty_voter_set() {
    TestState::default().build_and_execute(|| {
        let _guard = EmptyReviewVoterSetGuard::new(true);
        assert!(Pallet::<Test>::do_try_state().is_err());
    });
}

#[test]
fn try_state_rejects_some_empty_proposer_set() {
    TestState::default().build_and_execute(|| {
        let mut t = passorfail_track(0);
        t.info.proposer_set = Some(MemberSet::Union(vec![]));
        let _guard = OverrideTracksGuard::new(vec![t]);
        assert!(Pallet::<Test>::do_try_state().is_err());
    });
}

#[test]
fn try_state_accepts_none_proposer_set() {
    TestState::default().build_and_execute(|| {
        let mut t = passorfail_track(0);
        t.info.proposer_set = None;
        let _guard = OverrideTracksGuard::new(vec![t]);
        assert!(Pallet::<Test>::do_try_state().is_ok());
    });
}
