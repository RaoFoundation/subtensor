#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use frame_support::{assert_noop, assert_ok, sp_runtime::Perbill, traits::Hooks, weights::Weight};
use sp_core::U256;
use sp_runtime::{DispatchError, Saturating};
use subtensor_runtime_common::{OnPollCompleted, OnPollCreated, VoteTally};

use crate::{
    Error, Event as SignedVotingEvent, Pallet as SignedVotingPallet, PendingCleanup,
    SignedVoteTally, TallyOf, VoterSetOf, VotingFor, mock::*,
};

/// Loop `on_idle` with unlimited weight until `PendingCleanup` is empty.
/// Cursor-resume tests must use [`build_and_commit`] instead: the test
/// externality only progresses cleanup state across committed blocks.
fn drain_cleanup_queue() {
    let block = System::block_number();
    while !PendingCleanup::<Test>::get().is_empty() {
        SignedVotingPallet::<Test>::on_idle(block, Weight::MAX);
    }
}

/// Build a [`TestExternalities`], run `setup`, then commit so subsequent
/// `execute_with` blocks see the writes through the backend. Needed for
/// any test that calls `clear_prefix` with a non-trivial limit: the
/// limit ignores keys that live only in the overlay.
fn build_and_commit<F: FnOnce()>(setup: F) -> sp_io::TestExternalities {
    let mut ext = new_test_ext();
    ext.execute_with(setup);
    ext.commit_all().expect("commit_all");
    ext
}

#[test]
fn vote_aye_increments_ayes_and_emits_voted_event() {
    TestState::build_and_execute(|| {
        let alice = U256::from(1);
        start_poll(
            0,
            VotingScheme::Signed,
            vec![alice, U256::from(2), U256::from(3)],
        );

        assert_ok!(SignedVotingPallet::<Test>::vote(
            RuntimeOrigin::signed(alice),
            0u32,
            true,
        ));

        let tally = TallyOf::<Test>::get(0u32).unwrap();
        assert_eq!(tally.ayes, 1);
        assert_eq!(tally.nays, 0);
        assert_eq!(tally.total, 3);
        assert_eq!(VotingFor::<Test>::get(0u32, alice), Some(true));

        assert_eq!(
            signed_voting_events().last(),
            Some(&SignedVotingEvent::Voted {
                who: alice,
                poll_index: 0,
                approve: true,
                tally,
            })
        );
    });
}

#[test]
fn vote_nay_increments_nays() {
    TestState::build_and_execute(|| {
        let alice = U256::from(1);
        start_poll(0, VotingScheme::Signed, vec![alice, U256::from(2)]);

        assert_ok!(SignedVotingPallet::<Test>::vote(
            RuntimeOrigin::signed(alice),
            0u32,
            false,
        ));

        let tally = TallyOf::<Test>::get(0u32).unwrap();
        assert_eq!(tally.nays, 1);
        assert_eq!(VotingFor::<Test>::get(0u32, alice), Some(false));
    });
}

#[test]
fn vote_can_flip_aye_nay_aye() {
    TestState::build_and_execute(|| {
        let alice = U256::from(1);
        start_poll(0, VotingScheme::Signed, vec![alice, U256::from(2)]);

        assert_ok!(SignedVotingPallet::<Test>::vote(
            RuntimeOrigin::signed(alice),
            0u32,
            true,
        ));
        assert_eq!(
            (
                TallyOf::<Test>::get(0u32).unwrap().ayes,
                TallyOf::<Test>::get(0u32).unwrap().nays
            ),
            (1, 0)
        );

        assert_ok!(SignedVotingPallet::<Test>::vote(
            RuntimeOrigin::signed(alice),
            0u32,
            false,
        ));
        assert_eq!(
            (
                TallyOf::<Test>::get(0u32).unwrap().ayes,
                TallyOf::<Test>::get(0u32).unwrap().nays
            ),
            (0, 1)
        );

        assert_ok!(SignedVotingPallet::<Test>::vote(
            RuntimeOrigin::signed(alice),
            0u32,
            true,
        ));
        assert_eq!(
            (
                TallyOf::<Test>::get(0u32).unwrap().ayes,
                TallyOf::<Test>::get(0u32).unwrap().nays
            ),
            (1, 0)
        );
    });
}

#[test]
fn vote_aggregates_across_distinct_voters() {
    TestState::build_and_execute(|| {
        let alice = U256::from(1);
        let bob = U256::from(2);
        let charlie = U256::from(3);
        start_poll(0, VotingScheme::Signed, vec![alice, bob, charlie]);

        assert_ok!(SignedVotingPallet::<Test>::vote(
            RuntimeOrigin::signed(alice),
            0u32,
            true,
        ));
        assert_ok!(SignedVotingPallet::<Test>::vote(
            RuntimeOrigin::signed(bob),
            0u32,
            false,
        ));
        assert_ok!(SignedVotingPallet::<Test>::vote(
            RuntimeOrigin::signed(charlie),
            0u32,
            true,
        ));

        let tally = TallyOf::<Test>::get(0u32).unwrap();
        assert_eq!((tally.ayes, tally.nays, tally.total), (2, 1, 3));
    });
}

#[test]
fn vote_invokes_polls_on_tally_updated_with_perbill_ratios() {
    TestState::build_and_execute(|| {
        let alice = U256::from(1);
        start_poll(
            0,
            VotingScheme::Signed,
            vec![alice, U256::from(2), U256::from(3)],
        );

        assert_ok!(SignedVotingPallet::<Test>::vote(
            RuntimeOrigin::signed(alice),
            0u32,
            true,
        ));

        let updates = take_tally_updates();
        assert_eq!(updates.len(), 1);
        let (idx, tally) = &updates[0];
        assert_eq!(*idx, 0);
        assert_eq!(tally.approval, Perbill::from_rational(1u32, 3u32));
        assert_eq!(tally.rejection, Perbill::zero());
        assert_eq!(
            tally.abstention,
            Perbill::one().saturating_sub(tally.approval),
        );
        assert_eq!(
            tally.approval + tally.rejection + tally.abstention,
            Perbill::one(),
        );
    });
}

#[test]
fn vote_rejects_root_origin() {
    TestState::build_and_execute(|| {
        start_poll(0, VotingScheme::Signed, vec![U256::from(1)]);

        assert_noop!(
            SignedVotingPallet::<Test>::vote(RuntimeOrigin::root(), 0u32, true),
            DispatchError::BadOrigin
        );
    });
}

#[test]
fn vote_rejects_completed_poll_with_poll_not_ongoing() {
    TestState::build_and_execute(|| {
        let alice = U256::from(1);
        start_poll(0, VotingScheme::Signed, vec![alice]);
        complete_poll(0);

        assert_noop!(
            SignedVotingPallet::<Test>::vote(RuntimeOrigin::signed(alice), 0u32, true),
            Error::<Test>::PollNotOngoing
        );
    });
}

#[test]
fn vote_rejects_unknown_poll_with_poll_not_ongoing() {
    TestState::build_and_execute(|| {
        assert_noop!(
            SignedVotingPallet::<Test>::vote(RuntimeOrigin::signed(U256::from(1)), 999u32, true),
            Error::<Test>::PollNotOngoing
        );
    });
}

#[test]
fn vote_rejects_poll_with_mismatched_scheme() {
    TestState::build_and_execute(|| {
        let alice = U256::from(1);
        start_poll(0, VotingScheme::Anonymous, vec![alice]);

        assert_noop!(
            SignedVotingPallet::<Test>::vote(RuntimeOrigin::signed(alice), 0u32, true),
            Error::<Test>::InvalidVotingScheme
        );
    });
}

#[test]
fn vote_rejects_non_member_with_not_in_voter_set() {
    TestState::build_and_execute(|| {
        let mallory = U256::from(999);
        start_poll(0, VotingScheme::Signed, vec![U256::from(1), U256::from(2)]);

        assert_noop!(
            SignedVotingPallet::<Test>::vote(RuntimeOrigin::signed(mallory), 0u32, true),
            Error::<Test>::NotInVoterSet
        );
    });
}

#[test]
fn vote_rejects_duplicate_in_same_direction() {
    TestState::build_and_execute(|| {
        let alice = U256::from(1);
        start_poll(0, VotingScheme::Signed, vec![alice]);

        assert_ok!(SignedVotingPallet::<Test>::vote(
            RuntimeOrigin::signed(alice),
            0u32,
            true,
        ));

        assert_noop!(
            SignedVotingPallet::<Test>::vote(RuntimeOrigin::signed(alice), 0u32, true),
            Error::<Test>::DuplicateVote
        );

        let tally = TallyOf::<Test>::get(0u32).unwrap();
        assert_eq!((tally.ayes, tally.nays), (1, 0));
    });
}

#[test]
fn rotated_out_member_can_still_vote_until_poll_ends() {
    TestState::build_and_execute(|| {
        let alice = U256::from(1);
        start_poll(0, VotingScheme::Signed, vec![alice, U256::from(2)]);

        rotate_voter_out(0, alice);

        assert_ok!(SignedVotingPallet::<Test>::vote(
            RuntimeOrigin::signed(alice),
            0u32,
            true,
        ));
        assert_eq!(VotingFor::<Test>::get(0u32, alice), Some(true));
    });
}

#[test]
fn rotated_in_member_cannot_vote_on_poll_created_before_they_joined() {
    TestState::build_and_execute(|| {
        let alice = U256::from(1);
        let newcomer = U256::from(42);
        start_poll(0, VotingScheme::Signed, vec![alice]);

        rotate_voter_in(0, newcomer);

        assert_noop!(
            SignedVotingPallet::<Test>::vote(RuntimeOrigin::signed(newcomer), 0u32, true),
            Error::<Test>::NotInVoterSet
        );
    });
}

#[test]
fn rotated_out_member_can_flip_their_vote() {
    TestState::build_and_execute(|| {
        let alice = U256::from(1);
        start_poll(0, VotingScheme::Signed, vec![alice, U256::from(2)]);

        assert_ok!(SignedVotingPallet::<Test>::vote(
            RuntimeOrigin::signed(alice),
            0u32,
            true,
        ));
        rotate_voter_out(0, alice);

        assert_ok!(SignedVotingPallet::<Test>::vote(
            RuntimeOrigin::signed(alice),
            0u32,
            false,
        ));
        let tally = TallyOf::<Test>::get(0u32).unwrap();
        assert_eq!((tally.ayes, tally.nays), (0, 1));
        assert_eq!(VotingFor::<Test>::get(0u32, alice), Some(false));
    });
}

#[test]
fn remove_vote_clears_aye_and_emits_vote_removed_event() {
    TestState::build_and_execute(|| {
        let alice = U256::from(1);
        start_poll(0, VotingScheme::Signed, vec![alice, U256::from(2)]);

        assert_ok!(SignedVotingPallet::<Test>::vote(
            RuntimeOrigin::signed(alice),
            0u32,
            true,
        ));
        assert_ok!(SignedVotingPallet::<Test>::remove_vote(
            RuntimeOrigin::signed(alice),
            0u32,
        ));

        let tally = TallyOf::<Test>::get(0u32).unwrap();
        assert_eq!((tally.ayes, tally.nays, tally.total), (0, 0, 2));
        assert_eq!(VotingFor::<Test>::get(0u32, alice), None);

        assert_eq!(
            signed_voting_events().last(),
            Some(&SignedVotingEvent::VoteRemoved {
                who: alice,
                poll_index: 0,
                tally,
            })
        );
    });
}

#[test]
fn remove_vote_clears_nay() {
    TestState::build_and_execute(|| {
        let alice = U256::from(1);
        start_poll(0, VotingScheme::Signed, vec![alice, U256::from(2)]);

        assert_ok!(SignedVotingPallet::<Test>::vote(
            RuntimeOrigin::signed(alice),
            0u32,
            false,
        ));
        assert_ok!(SignedVotingPallet::<Test>::remove_vote(
            RuntimeOrigin::signed(alice),
            0u32,
        ));

        let tally = TallyOf::<Test>::get(0u32).unwrap();
        assert_eq!(tally.nays, 0);
        assert_eq!(VotingFor::<Test>::get(0u32, alice), None);
    });
}

#[test]
fn remove_vote_rejects_root_origin() {
    TestState::build_and_execute(|| {
        start_poll(0, VotingScheme::Signed, vec![U256::from(1)]);

        assert_noop!(
            SignedVotingPallet::<Test>::remove_vote(RuntimeOrigin::root(), 0u32),
            DispatchError::BadOrigin
        );
    });
}

#[test]
fn remove_vote_rejects_completed_poll() {
    TestState::build_and_execute(|| {
        let alice = U256::from(1);
        start_poll(0, VotingScheme::Signed, vec![alice]);
        assert_ok!(SignedVotingPallet::<Test>::vote(
            RuntimeOrigin::signed(alice),
            0u32,
            true,
        ));
        complete_poll(0);

        assert_noop!(
            SignedVotingPallet::<Test>::remove_vote(RuntimeOrigin::signed(alice), 0u32),
            Error::<Test>::PollNotOngoing
        );
    });
}

#[test]
fn remove_vote_rejects_poll_with_mismatched_scheme() {
    TestState::build_and_execute(|| {
        let alice = U256::from(1);
        start_poll(0, VotingScheme::Anonymous, vec![alice]);

        assert_noop!(
            SignedVotingPallet::<Test>::remove_vote(RuntimeOrigin::signed(alice), 0u32),
            Error::<Test>::InvalidVotingScheme
        );
    });
}

#[test]
fn remove_vote_rejects_non_member() {
    TestState::build_and_execute(|| {
        let mallory = U256::from(999);
        start_poll(0, VotingScheme::Signed, vec![U256::from(1)]);

        assert_noop!(
            SignedVotingPallet::<Test>::remove_vote(RuntimeOrigin::signed(mallory), 0u32),
            Error::<Test>::NotInVoterSet
        );
    });
}

#[test]
fn remove_vote_rejects_voter_who_never_voted() {
    TestState::build_and_execute(|| {
        let alice = U256::from(1);
        start_poll(0, VotingScheme::Signed, vec![alice]);

        assert_noop!(
            SignedVotingPallet::<Test>::remove_vote(RuntimeOrigin::signed(alice), 0u32),
            Error::<Test>::VoteNotFound
        );
    });
}

#[test]
fn remove_vote_succeeds_for_voter_rotated_out_after_creation() {
    TestState::build_and_execute(|| {
        let alice = U256::from(1);
        start_poll(0, VotingScheme::Signed, vec![alice, U256::from(2)]);

        assert_ok!(SignedVotingPallet::<Test>::vote(
            RuntimeOrigin::signed(alice),
            0u32,
            true,
        ));
        rotate_voter_out(0, alice);

        assert_ok!(SignedVotingPallet::<Test>::remove_vote(
            RuntimeOrigin::signed(alice),
            0u32,
        ));
        assert_eq!(VotingFor::<Test>::get(0u32, alice), None);
    });
}

#[test]
fn on_poll_created_initializes_tally_with_voter_set_size() {
    TestState::build_and_execute(|| {
        let voters: Vec<U256> = (1..=5u32).map(U256::from).collect();
        start_poll(0, VotingScheme::Signed, voters);

        let tally = TallyOf::<Test>::get(0u32).unwrap();
        assert_eq!(
            tally,
            SignedVoteTally {
                ayes: 0,
                nays: 0,
                total: 5,
            }
        );
    });
}

#[test]
fn on_poll_created_snapshots_voter_set_into_voter_set_of() {
    TestState::build_and_execute(|| {
        let voters: Vec<U256> = (1..=4u32).map(U256::from).collect();
        start_poll(0, VotingScheme::Signed, voters.clone());

        let snapshot = VoterSetOf::<Test>::get(0u32).expect("snapshot stored");
        assert_eq!(snapshot.to_vec(), voters);
    });
}

/// Defense-in-depth path. The runtime's compile-time bound checks and
/// `pallet-referenda::submit`'s `EmptyVoterSet` guard should make this
/// unreachable, but if the producer ever hands over an oversized set
/// the pallet falls back to an empty snapshot rather than panicking.
#[test]
fn on_poll_created_with_oversized_voter_set_falls_back_to_empty() {
    TestState::build_and_execute(|| {
        let cap = TestMaxVoterSetSize::get();
        let voters: Vec<U256> = (1..=(cap + 1)).map(|i| U256::from(i as u64)).collect();
        start_poll(0, VotingScheme::Signed, voters);

        let snapshot = VoterSetOf::<Test>::get(0u32).expect("snapshot stored");
        assert!(snapshot.is_empty());
        assert_eq!(TallyOf::<Test>::get(0u32).unwrap().total, 0);
    });
}

#[test]
fn on_poll_created_twice_does_not_clobber_existing_tally() {
    TestState::build_and_execute(|| {
        let alice = U256::from(1);
        let bob = U256::from(2);
        start_poll(0, VotingScheme::Signed, vec![alice, bob]);

        assert_ok!(SignedVotingPallet::<Test>::vote(
            RuntimeOrigin::signed(alice),
            0u32,
            true,
        ));
        let tally_before = TallyOf::<Test>::get(0u32).expect("tally seeded");
        assert_eq!(tally_before.ayes, 1);

        <SignedVotingPallet<Test> as OnPollCreated<u32>>::on_poll_created(0u32);

        let tally_after = TallyOf::<Test>::get(0u32).expect("tally preserved");
        assert_eq!(tally_after, tally_before);
        assert_eq!(VotingFor::<Test>::get(0u32, alice), Some(true));
    });
}

#[test]
fn on_poll_created_skips_polls_with_mismatched_scheme() {
    TestState::build_and_execute(|| {
        let alice = U256::from(1);
        start_poll(0, VotingScheme::Anonymous, vec![alice]);

        assert!(TallyOf::<Test>::get(0u32).is_none());
        assert!(VoterSetOf::<Test>::get(0u32).is_none());
    });
}

#[test]
fn on_poll_created_sorts_and_dedups_voter_set() {
    TestState::build_and_execute(|| {
        let alice = U256::from(1);
        let bob = U256::from(2);
        let carol = U256::from(3);
        start_poll(0, VotingScheme::Signed, vec![carol, bob, alice, bob, carol]);

        let snapshot = VoterSetOf::<Test>::get(0u32).expect("snapshot stored");
        assert_eq!(snapshot.to_vec(), vec![alice, bob, carol]);
        assert_eq!(TallyOf::<Test>::get(0u32).unwrap().total, 3);
    });
}

#[test]
fn tally_total_is_immune_to_membership_changes_after_creation() {
    TestState::build_and_execute(|| {
        let alice = U256::from(1);
        let bob = U256::from(2);
        start_poll(0, VotingScheme::Signed, vec![alice, bob]);
        let total_at_creation = TallyOf::<Test>::get(0u32).unwrap().total;
        assert_eq!(total_at_creation, 2);

        rotate_voter_out(0, alice);
        rotate_voter_in(0, U256::from(99));

        assert_eq!(TallyOf::<Test>::get(0u32).unwrap().total, total_at_creation);
    });
}

#[test]
fn on_poll_completed_synchronously_clears_tally_and_voter_set() {
    TestState::build_and_execute(|| {
        let alice = U256::from(1);
        let bob = U256::from(2);
        start_poll(0, VotingScheme::Signed, vec![alice, bob]);

        assert_ok!(SignedVotingPallet::<Test>::vote(
            RuntimeOrigin::signed(alice),
            0u32,
            true,
        ));
        assert_ok!(SignedVotingPallet::<Test>::vote(
            RuntimeOrigin::signed(bob),
            0u32,
            false,
        ));

        complete_poll(0);

        assert!(TallyOf::<Test>::get(0u32).is_none());
        assert!(VoterSetOf::<Test>::get(0u32).is_none());
    });
}

#[test]
fn on_poll_completed_enqueues_voting_for_for_lazy_cleanup() {
    TestState::build_and_execute(|| {
        let alice = U256::from(1);
        start_poll(0, VotingScheme::Signed, vec![alice, U256::from(2)]);
        assert_ok!(SignedVotingPallet::<Test>::vote(
            RuntimeOrigin::signed(alice),
            0u32,
            true,
        ));

        complete_poll(0);

        let queue = PendingCleanup::<Test>::get();
        assert_eq!(queue.len(), 1);
        assert_eq!(queue[0].0, 0u32);
        assert!(queue[0].1.is_none(), "fresh enqueue carries no cursor");
        assert_eq!(VotingFor::<Test>::get(0u32, alice), Some(true));
    });
}

#[test]
fn on_poll_completed_twice_does_not_duplicate_cleanup_queue() {
    TestState::build_and_execute(|| {
        let alice = U256::from(1);
        start_poll(0, VotingScheme::Signed, vec![alice]);
        complete_poll(0);
        assert_eq!(PendingCleanup::<Test>::get().len(), 1);

        <SignedVotingPallet<Test> as OnPollCompleted<u32>>::on_poll_completed(0u32);
        assert_eq!(PendingCleanup::<Test>::get().len(), 1);
    });
}

#[test]
fn on_poll_completed_no_ops_when_no_local_tally() {
    TestState::build_and_execute(|| {
        let alice = U256::from(1);
        start_poll(0, VotingScheme::Anonymous, vec![alice]);

        complete_poll(0);
        assert!(PendingCleanup::<Test>::get().is_empty());
    });
}

#[test]
fn on_poll_completed_emits_cleanup_queue_full_and_leaks_voting_for() {
    TestState::build_and_execute(|| {
        let cap = TestMaxPendingCleanup::get();
        for i in 0..cap {
            start_poll(i, VotingScheme::Signed, vec![U256::from(i as u64 + 1)]);
            complete_poll(i);
        }
        let extra = cap;
        let leaker = U256::from(99);
        start_poll(extra, VotingScheme::Signed, vec![leaker]);
        assert_ok!(SignedVotingPallet::<Test>::vote(
            RuntimeOrigin::signed(leaker),
            extra,
            true,
        ));
        complete_poll(extra);

        let events = signed_voting_events();
        assert!(
            events.iter().any(|e| matches!(
                e,
                SignedVotingEvent::CleanupQueueFull { poll_index } if *poll_index == extra
            )),
            "CleanupQueueFull event must fire for poll {}",
            extra
        );
        assert_eq!(PendingCleanup::<Test>::get().len(), cap as usize);
        assert_eq!(
            VotingFor::<Test>::get(extra, leaker),
            Some(true),
            "overflow path must leak VotingFor for the rejected poll",
        );
    });
}

/// Stress check at 200 voters, well past any track's `MaxVoterSetSize`.
/// Catches regressions where the cleanup queue or its drain loop
/// silently drops entries.
#[test]
fn drain_cleanup_queue_clears_all_voting_for_entries_for_completed_polls() {
    TestState::build_and_execute(|| {
        let voters: Vec<U256> = (1..=200u32).map(U256::from).collect();
        start_poll(0, VotingScheme::Signed, voters.clone());
        for v in &voters {
            assert_ok!(SignedVotingPallet::<Test>::vote(
                RuntimeOrigin::signed(*v),
                0u32,
                true,
            ));
        }

        complete_poll(0);
        drain_cleanup_queue();

        for v in &voters {
            assert_eq!(VotingFor::<Test>::get(0u32, *v), None);
        }
        assert!(PendingCleanup::<Test>::get().is_empty());
    });
}

/// One drain pass clears at most `CleanupChunkSize` entries and
/// persists the resume cursor on the queue head, so a busy chain
/// cannot starve cleanup of bounded weight.
#[test]
fn on_idle_clears_one_chunk_per_pass_and_stores_cursor() {
    use crate::weights::WeightInfo as _;

    let voters: Vec<U256> = (1..=10u32).map(U256::from).collect();
    let mut ext = build_and_commit(|| {
        start_poll(0, VotingScheme::Signed, voters.clone());
        for v in &voters {
            assert_ok!(SignedVotingPallet::<Test>::vote(
                RuntimeOrigin::signed(*v),
                0u32,
                true,
            ));
        }
        complete_poll(0);
    });

    ext.execute_with(|| {
        let chunk = TestCleanupChunkSize::get();
        let one_step = <<Test as crate::Config>::WeightInfo>::idle_cleanup_chunk(chunk);
        let budget = one_step.saturating_add(one_step.saturating_div(2));

        SignedVotingPallet::<Test>::on_idle(System::block_number(), budget);

        let remaining = voters
            .iter()
            .filter(|v| VotingFor::<Test>::get(0u32, **v).is_some())
            .count();
        assert_eq!(remaining, voters.len() - chunk as usize);

        let queue = PendingCleanup::<Test>::get();
        assert_eq!(queue.len(), 1);
        assert_eq!(queue[0].0, 0u32);
        assert!(
            queue[0].1.is_some(),
            "cursor must be persisted after a partial clear"
        );
    });
}

/// Successive drain passes resume from the persisted cursor. Each pass
/// runs in its own committed externality so `clear_prefix`'s cursor sees
/// real backend state, not just the in-block overlay.
#[test]
fn successive_idle_passes_resume_via_cursor_until_drained() {
    use crate::weights::WeightInfo as _;

    let voters: Vec<U256> = (1..=10u32).map(U256::from).collect();
    let mut ext = build_and_commit(|| {
        start_poll(0, VotingScheme::Signed, voters.clone());
        for v in &voters {
            assert_ok!(SignedVotingPallet::<Test>::vote(
                RuntimeOrigin::signed(*v),
                0u32,
                true,
            ));
        }
        complete_poll(0);
    });

    let chunk = TestCleanupChunkSize::get();
    let one_step = <<Test as crate::Config>::WeightInfo>::idle_cleanup_chunk(chunk);
    let budget = one_step + (one_step / 2);

    for _ in 0..3 {
        ext.execute_with(|| {
            SignedVotingPallet::<Test>::on_idle(System::block_number(), budget);
        });
        ext.commit_all().expect("commit_all");
    }

    ext.execute_with(|| {
        let stored = VotingFor::<Test>::iter_prefix(0u32).count();
        assert_eq!(stored, 0, "all VotingFor entries must be drained");
        assert!(PendingCleanup::<Test>::get().is_empty());
    });
}

#[test]
fn idle_drain_finishes_head_poll_before_starting_next() {
    let voters_a: Vec<U256> = (1..=8u32).map(U256::from).collect();
    let voters_b: Vec<U256> = (101..=108u32).map(U256::from).collect();
    let mut ext = build_and_commit(|| {
        start_poll(0, VotingScheme::Signed, voters_a.clone());
        start_poll(1, VotingScheme::Signed, voters_b.clone());
        for v in &voters_a {
            assert_ok!(SignedVotingPallet::<Test>::vote(
                RuntimeOrigin::signed(*v),
                0u32,
                true,
            ));
        }
        for v in &voters_b {
            assert_ok!(SignedVotingPallet::<Test>::vote(
                RuntimeOrigin::signed(*v),
                1u32,
                true,
            ));
        }
        complete_poll(0);
        complete_poll(1);
    });

    ext.execute_with(|| {
        use crate::weights::WeightInfo as _;
        let chunk = TestCleanupChunkSize::get();
        let one_step = <<Test as crate::Config>::WeightInfo>::idle_cleanup_chunk(chunk);
        let single_budget = one_step.saturating_add(one_step.saturating_div(2));

        SignedVotingPallet::<Test>::on_idle(System::block_number(), single_budget);

        let a_remaining = voters_a
            .iter()
            .filter(|v| VotingFor::<Test>::get(0u32, **v).is_some())
            .count();
        let b_remaining = voters_b
            .iter()
            .filter(|v| VotingFor::<Test>::get(1u32, **v).is_some())
            .count();
        assert_eq!(a_remaining, voters_a.len() - chunk as usize);
        assert_eq!(b_remaining, voters_b.len(), "poll 1 must not be touched");

        let queue = PendingCleanup::<Test>::get();
        assert_eq!(queue.len(), 2);
        assert_eq!(queue[0].0, 0u32, "poll 0 still at head");
        assert_eq!(queue[1].0, 1u32);
    });
}

#[test]
fn on_idle_is_noop_when_weight_below_one_drain_step() {
    use crate::weights::WeightInfo as _;

    TestState::build_and_execute(|| {
        let alice = U256::from(1);
        start_poll(0, VotingScheme::Signed, vec![alice, U256::from(2)]);
        assert_ok!(SignedVotingPallet::<Test>::vote(
            RuntimeOrigin::signed(alice),
            0u32,
            true,
        ));
        complete_poll(0);

        let chunk = TestCleanupChunkSize::get();
        let one_step = <<Test as crate::Config>::WeightInfo>::idle_cleanup_chunk(chunk);
        let starved = one_step.saturating_div(2);

        SignedVotingPallet::<Test>::on_idle(System::block_number(), starved);

        assert_eq!(PendingCleanup::<Test>::get().len(), 1);
        assert_eq!(VotingFor::<Test>::get(0u32, alice), Some(true));
    });
}

/// `on_idle` with an empty queue consumes only the upfront 1-read /
/// 1-write reservation. The mock uses `RocksDbWeight` so this catches
/// regressions that the default `DbWeight = ()` would silently mask.
#[test]
fn on_idle_with_empty_queue_consumes_only_entry_cost() {
    TestState::build_and_execute(|| {
        let entry_cost = <Test as frame_system::Config>::DbWeight::get().reads_writes(1, 1);
        let consumed = SignedVotingPallet::<Test>::on_idle(System::block_number(), Weight::MAX);
        assert_eq!(consumed, entry_cost);
    });
}

#[test]
fn on_idle_consumes_nothing_when_budget_below_entry_cost() {
    TestState::build_and_execute(|| {
        let consumed = SignedVotingPallet::<Test>::on_idle(System::block_number(), Weight::zero());
        assert_eq!(consumed, Weight::zero());
    });
}

#[test]
fn tally_conversion_computes_perbill_ratios() {
    let tally = SignedVoteTally {
        ayes: 1,
        nays: 2,
        total: 10,
    };
    let vote_tally: VoteTally = tally.into();

    assert_eq!(vote_tally.approval, Perbill::from_rational(1u32, 10u32));
    assert_eq!(vote_tally.rejection, Perbill::from_rational(2u32, 10u32));
    assert_eq!(vote_tally.abstention, Perbill::from_rational(7u32, 10u32));
}

#[test]
fn tally_conversion_saturates_approval_when_all_aye() {
    let tally = SignedVoteTally {
        ayes: 3,
        nays: 0,
        total: 3,
    };
    let vote_tally: VoteTally = tally.into();

    assert_eq!(vote_tally.approval, Perbill::one());
    assert_eq!(vote_tally.rejection, Perbill::zero());
    assert_eq!(vote_tally.abstention, Perbill::zero());
}

/// `Perbill::from_rational(_, 0)` returns 100%, so a naive conversion
/// of a zero-total tally would yield approval + rejection + abstention
/// = 300%. The short-circuit to `default()` avoids that.
#[test]
fn tally_conversion_short_circuits_zero_total_to_default() {
    let tally = SignedVoteTally {
        ayes: 0,
        nays: 0,
        total: 0,
    };
    let vote_tally: VoteTally = tally.into();

    assert_eq!(vote_tally, VoteTally::default());
    assert_eq!(vote_tally.approval, Perbill::zero());
    assert_eq!(vote_tally.rejection, Perbill::zero());
    assert_eq!(vote_tally.abstention, Perbill::one());
}

#[test]
fn tally_conversion_saturates_rejection_when_all_nay() {
    let tally = SignedVoteTally {
        ayes: 0,
        nays: 3,
        total: 3,
    };
    let vote_tally: VoteTally = tally.into();

    assert_eq!(vote_tally.approval, Perbill::zero());
    assert_eq!(vote_tally.rejection, Perbill::one());
    assert_eq!(vote_tally.abstention, Perbill::zero());
}

#[test]
fn integrity_test_passes_for_default_config() {
    SignedVotingPallet::<Test>::integrity_test();
}

#[test]
#[should_panic(expected = "CleanupChunkSize must be non-zero")]
fn integrity_test_panics_when_cleanup_chunk_size_is_zero() {
    let _g = CleanupChunkSizeGuard::new(0);
    SignedVotingPallet::<Test>::integrity_test();
}

#[test]
#[should_panic(expected = "MaxPendingCleanup must be non-zero")]
fn integrity_test_panics_when_max_pending_cleanup_is_zero() {
    let _g = MaxPendingCleanupGuard::new(0);
    SignedVotingPallet::<Test>::integrity_test();
}

#[test]
#[should_panic(expected = "MaxVoterSetSize must be non-zero")]
fn integrity_test_panics_when_max_voter_set_size_is_zero() {
    let _g = MaxVoterSetSizeGuard::new(0);
    SignedVotingPallet::<Test>::integrity_test();
}

#[test]
fn vote_returns_poll_not_found_when_producer_reports_no_scheme() {
    TestState::build_and_execute(|| {
        let alice = U256::from(1);
        start_poll(0, VotingScheme::Signed, vec![alice]);
        force_scheme_none(0);

        assert_noop!(
            SignedVotingPallet::<Test>::vote(RuntimeOrigin::signed(alice), 0u32, true),
            Error::<Test>::PollNotFound
        );
    });
}

#[test]
fn vote_returns_tally_missing_on_internal_inconsistency() {
    TestState::build_and_execute(|| {
        let alice = U256::from(1);
        start_poll(0, VotingScheme::Signed, vec![alice]);
        TallyOf::<Test>::remove(0u32);

        assert_noop!(
            SignedVotingPallet::<Test>::vote(RuntimeOrigin::signed(alice), 0u32, true),
            Error::<Test>::TallyMissing
        );
    });
}

#[test]
fn remove_vote_returns_tally_missing_on_internal_inconsistency() {
    TestState::build_and_execute(|| {
        let alice = U256::from(1);
        start_poll(0, VotingScheme::Signed, vec![alice]);
        assert_ok!(SignedVotingPallet::<Test>::vote(
            RuntimeOrigin::signed(alice),
            0u32,
            true,
        ));
        TallyOf::<Test>::remove(0u32);

        assert_noop!(
            SignedVotingPallet::<Test>::remove_vote(RuntimeOrigin::signed(alice), 0u32),
            Error::<Test>::TallyMissing
        );
    });
}

#[test]
fn vote_returns_voter_set_missing_on_internal_inconsistency() {
    TestState::build_and_execute(|| {
        let alice = U256::from(1);
        start_poll(0, VotingScheme::Signed, vec![alice]);
        VoterSetOf::<Test>::remove(0u32);

        assert_noop!(
            SignedVotingPallet::<Test>::vote(RuntimeOrigin::signed(alice), 0u32, true),
            Error::<Test>::VoterSetMissing
        );
    });
}

#[test]
fn remove_vote_invokes_polls_on_tally_updated() {
    TestState::build_and_execute(|| {
        let alice = U256::from(1);
        start_poll(
            0,
            VotingScheme::Signed,
            vec![alice, U256::from(2), U256::from(3)],
        );
        assert_ok!(SignedVotingPallet::<Test>::vote(
            RuntimeOrigin::signed(alice),
            0u32,
            true,
        ));
        let _ = take_tally_updates();

        assert_ok!(SignedVotingPallet::<Test>::remove_vote(
            RuntimeOrigin::signed(alice),
            0u32,
        ));

        let updates = take_tally_updates();
        assert_eq!(updates.len(), 1);
        let (idx, tally) = &updates[0];
        assert_eq!(*idx, 0);
        assert_eq!(tally.approval, Perbill::zero());
        assert_eq!(tally.rejection, Perbill::zero());
        assert_eq!(tally.abstention, Perbill::one());
    });
}
