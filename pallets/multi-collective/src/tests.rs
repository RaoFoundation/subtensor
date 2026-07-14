#![allow(clippy::unwrap_used, clippy::expect_used)]

use frame_support::{BoundedVec, assert_noop, assert_ok, traits::Hooks, weights::Weight};
use sp_core::U256;
use sp_runtime::DispatchError;

use crate::{
    Collective, CollectiveInfo, CollectiveInspect, Error, Event as CollectiveEvent, OnNewTerm,
    Pallet as MultiCollective, mock::*,
};

#[test]
fn add_member_happy_path() {
    TestState::build_and_execute(|| {
        let mid = U256::from(5);
        let head = U256::from(2);
        let tail = U256::from(8);
        let between = U256::from(4);

        // Exercises the four insertion positions that `binary_search` can
        // return: empty list, before the first element, after the last,
        // and into the middle. A regression replacing the sorted insert
        // with `push` would only be caught by the head and middle cases.
        assert_ok!(MultiCollective::<Test>::add_member(
            RuntimeOrigin::root(),
            CollectiveId::Alpha,
            mid,
        ));
        assert_eq!(
            MultiCollective::<Test>::members_of(CollectiveId::Alpha),
            vec![mid]
        );
        assert!(MultiCollective::<Test>::is_member(
            CollectiveId::Alpha,
            &mid
        ));

        assert_ok!(MultiCollective::<Test>::add_member(
            RuntimeOrigin::root(),
            CollectiveId::Alpha,
            head,
        ));
        assert_eq!(
            MultiCollective::<Test>::members_of(CollectiveId::Alpha),
            vec![head, mid]
        );

        assert_ok!(MultiCollective::<Test>::add_member(
            RuntimeOrigin::root(),
            CollectiveId::Alpha,
            tail,
        ));
        assert_eq!(
            MultiCollective::<Test>::members_of(CollectiveId::Alpha),
            vec![head, mid, tail]
        );

        assert_ok!(MultiCollective::<Test>::add_member(
            RuntimeOrigin::root(),
            CollectiveId::Alpha,
            between,
        ));
        assert_eq!(
            MultiCollective::<Test>::members_of(CollectiveId::Alpha),
            vec![head, between, mid, tail]
        );

        assert_eq!(
            MultiCollective::<Test>::member_count(CollectiveId::Alpha),
            4
        );

        assert_eq!(
            multi_collective_events(),
            vec![
                CollectiveEvent::MemberAdded {
                    collective_id: CollectiveId::Alpha,
                    who: mid,
                },
                CollectiveEvent::MemberAdded {
                    collective_id: CollectiveId::Alpha,
                    who: head,
                },
                CollectiveEvent::MemberAdded {
                    collective_id: CollectiveId::Alpha,
                    who: tail,
                },
                CollectiveEvent::MemberAdded {
                    collective_id: CollectiveId::Alpha,
                    who: between,
                },
            ]
        );
    });
}

#[test]
fn add_member_requires_origin() {
    TestState::build_and_execute(|| {
        let alice = U256::from(1);
        let caller = U256::from(999);

        assert_noop!(
            MultiCollective::<Test>::add_member(
                RuntimeOrigin::signed(caller),
                CollectiveId::Alpha,
                alice,
            ),
            DispatchError::BadOrigin
        );

        assert!(MultiCollective::<Test>::members_of(CollectiveId::Alpha).is_empty());
        assert!(multi_collective_events().is_empty());
    });
}

#[test]
fn add_member_fails_for_unknown_collective() {
    TestState::build_and_execute(|| {
        let alice = U256::from(1);

        assert_noop!(
            MultiCollective::<Test>::add_member(
                RuntimeOrigin::root(),
                CollectiveId::Unknown,
                alice,
            ),
            Error::<Test>::CollectiveNotFound
        );

        assert!(multi_collective_events().is_empty());
    });
}

#[test]
fn add_member_rejects_duplicate() {
    TestState::build_and_execute(|| {
        let alice = U256::from(1);

        assert_ok!(MultiCollective::<Test>::add_member(
            RuntimeOrigin::root(),
            CollectiveId::Alpha,
            alice,
        ));

        assert_noop!(
            MultiCollective::<Test>::add_member(RuntimeOrigin::root(), CollectiveId::Alpha, alice,),
            Error::<Test>::AlreadyMember
        );

        // Only one MemberAdded event; the failing call produced nothing.
        assert_eq!(
            multi_collective_events(),
            vec![CollectiveEvent::MemberAdded {
                collective_id: CollectiveId::Alpha,
                who: alice,
            }]
        );
        assert_eq!(
            MultiCollective::<Test>::member_count(CollectiveId::Alpha),
            1
        );
    });
}

#[test]
fn add_member_respects_info_max() {
    TestState::build_and_execute(|| {
        // Alpha declares max_members = Some(5). Fill it exactly to capacity.
        for i in 1..=5u32 {
            assert_ok!(MultiCollective::<Test>::add_member(
                RuntimeOrigin::root(),
                CollectiveId::Alpha,
                U256::from(i),
            ));
        }
        assert_eq!(
            MultiCollective::<Test>::member_count(CollectiveId::Alpha),
            5
        );

        assert_noop!(
            MultiCollective::<Test>::add_member(
                RuntimeOrigin::root(),
                CollectiveId::Alpha,
                U256::from(6),
            ),
            Error::<Test>::TooManyMembers
        );

        assert_eq!(
            MultiCollective::<Test>::member_count(CollectiveId::Alpha),
            5
        );
        // Exactly five events; nothing from the failing 6th.
        assert_eq!(multi_collective_events().len(), 5);
    });
}

#[test]
fn add_member_respects_storage_max_when_info_max_none() {
    TestState::build_and_execute(|| {
        // Gamma's `info.max_members` is None; only `T::MaxMembers = 32` applies.
        for i in 1..=32u32 {
            assert_ok!(MultiCollective::<Test>::add_member(
                RuntimeOrigin::root(),
                CollectiveId::Gamma,
                U256::from(i),
            ));
        }
        assert_eq!(
            MultiCollective::<Test>::member_count(CollectiveId::Gamma),
            32
        );

        // 33rd add fails via `try_insert` (BoundedVec bound) rather than the info cap.
        assert_noop!(
            MultiCollective::<Test>::add_member(
                RuntimeOrigin::root(),
                CollectiveId::Gamma,
                U256::from(33),
            ),
            Error::<Test>::TooManyMembers
        );

        assert_eq!(
            MultiCollective::<Test>::member_count(CollectiveId::Gamma),
            32
        );
        assert_eq!(multi_collective_events().len(), 32);
    });
}

#[test]
fn remove_member_happy_path() {
    TestState::build_and_execute(|| {
        let alice = U256::from(1);
        let bob = U256::from(2);
        let charlie = U256::from(3);

        for who in [alice, bob, charlie] {
            assert_ok!(MultiCollective::<Test>::add_member(
                RuntimeOrigin::root(),
                CollectiveId::Alpha,
                who,
            ));
        }

        // Remove from the middle.
        assert_ok!(MultiCollective::<Test>::remove_member(
            RuntimeOrigin::root(),
            CollectiveId::Alpha,
            bob,
        ));

        assert_eq!(
            MultiCollective::<Test>::members_of(CollectiveId::Alpha),
            vec![alice, charlie]
        );
        assert!(!MultiCollective::<Test>::is_member(
            CollectiveId::Alpha,
            &bob
        ));
        assert_eq!(
            MultiCollective::<Test>::member_count(CollectiveId::Alpha),
            2
        );

        // Remove from the head. A swap-remove would leave the list
        // unsorted (`[charlie, ...]` shifting via swap), so asserting
        // that the remaining tail stays in order discriminates against
        // that regression.
        assert_ok!(MultiCollective::<Test>::remove_member(
            RuntimeOrigin::root(),
            CollectiveId::Alpha,
            alice,
        ));

        assert_eq!(
            MultiCollective::<Test>::members_of(CollectiveId::Alpha),
            vec![charlie]
        );
        assert!(!MultiCollective::<Test>::is_member(
            CollectiveId::Alpha,
            &alice
        ));
        assert_eq!(
            MultiCollective::<Test>::member_count(CollectiveId::Alpha),
            1
        );

        assert_eq!(
            multi_collective_events().last(),
            Some(&CollectiveEvent::MemberRemoved {
                collective_id: CollectiveId::Alpha,
                who: alice,
            })
        );
    });
}

#[test]
fn remove_member_requires_origin() {
    TestState::build_and_execute(|| {
        let alice = U256::from(1);
        assert_ok!(MultiCollective::<Test>::add_member(
            RuntimeOrigin::root(),
            CollectiveId::Alpha,
            alice,
        ));

        assert_noop!(
            MultiCollective::<Test>::remove_member(
                RuntimeOrigin::signed(U256::from(999)),
                CollectiveId::Alpha,
                alice,
            ),
            DispatchError::BadOrigin
        );

        assert!(MultiCollective::<Test>::is_member(
            CollectiveId::Alpha,
            &alice
        ));
    });
}

#[test]
fn remove_member_fails_for_unknown_collective() {
    TestState::build_and_execute(|| {
        assert_noop!(
            MultiCollective::<Test>::remove_member(
                RuntimeOrigin::root(),
                CollectiveId::Unknown,
                U256::from(1),
            ),
            Error::<Test>::CollectiveNotFound
        );

        assert!(multi_collective_events().is_empty());
    });
}

#[test]
fn remove_member_rejects_non_member() {
    TestState::build_and_execute(|| {
        assert_noop!(
            MultiCollective::<Test>::remove_member(
                RuntimeOrigin::root(),
                CollectiveId::Alpha,
                U256::from(1),
            ),
            Error::<Test>::NotMember
        );

        assert!(multi_collective_events().is_empty());
    });
}

#[test]
fn remove_member_respects_min() {
    TestState::build_and_execute(|| {
        // Beta declares min_members = 2. Seed exactly to the floor.
        let alice = U256::from(1);
        let bob = U256::from(2);
        for who in [alice, bob] {
            assert_ok!(MultiCollective::<Test>::add_member(
                RuntimeOrigin::root(),
                CollectiveId::Beta,
                who,
            ));
        }

        assert_noop!(
            MultiCollective::<Test>::remove_member(
                RuntimeOrigin::root(),
                CollectiveId::Beta,
                alice,
            ),
            Error::<Test>::TooFewMembers
        );

        assert_eq!(MultiCollective::<Test>::member_count(CollectiveId::Beta), 2);
    });
}

#[test]
fn remove_member_allows_down_to_min() {
    TestState::build_and_execute(|| {
        // Beta has min_members = 2; seed with one above.
        let alice = U256::from(1);
        let bob = U256::from(2);
        let charlie = U256::from(3);
        for who in [alice, bob, charlie] {
            assert_ok!(MultiCollective::<Test>::add_member(
                RuntimeOrigin::root(),
                CollectiveId::Beta,
                who,
            ));
        }

        // Removing once leaves the collective at min_members; the check is
        // `len() > min_members` so post-removal len == min_members is allowed.
        assert_ok!(MultiCollective::<Test>::remove_member(
            RuntimeOrigin::root(),
            CollectiveId::Beta,
            charlie,
        ));

        assert_eq!(MultiCollective::<Test>::member_count(CollectiveId::Beta), 2);
        assert!(!MultiCollective::<Test>::is_member(
            CollectiveId::Beta,
            &charlie
        ));

        assert_eq!(
            multi_collective_events().last(),
            Some(&CollectiveEvent::MemberRemoved {
                collective_id: CollectiveId::Beta,
                who: charlie,
            })
        );
    });
}

#[test]
fn swap_member_happy_path() {
    TestState::build_and_execute(|| {
        let alice = U256::from(1);
        let bob = U256::from(2);
        let charlie = U256::from(3);
        let dave = U256::from(4);
        let zara = U256::from(10);

        for who in [alice, bob, charlie] {
            assert_ok!(MultiCollective::<Test>::add_member(
                RuntimeOrigin::root(),
                CollectiveId::Alpha,
                who,
            ));
        }

        // Swap the middle member for an account that sorts to the tail.
        assert_ok!(MultiCollective::<Test>::swap_member(
            RuntimeOrigin::root(),
            CollectiveId::Alpha,
            bob,
            dave,
        ));

        // Members are kept sorted: dave (4) goes after charlie (3).
        assert_eq!(
            MultiCollective::<Test>::members_of(CollectiveId::Alpha),
            vec![alice, charlie, dave]
        );
        assert!(!MultiCollective::<Test>::is_member(
            CollectiveId::Alpha,
            &bob
        ));
        assert!(MultiCollective::<Test>::is_member(
            CollectiveId::Alpha,
            &dave
        ));

        assert_eq!(
            multi_collective_events().last(),
            Some(&CollectiveEvent::MemberSwapped {
                collective_id: CollectiveId::Alpha,
                removed: bob,
                added: dave,
            })
        );

        // Swap the head member for an account that sorts to the tail.
        // A swap-remove regression on the remove side would leave the
        // resulting list unsorted, so this exercises both sides of the
        // sorted invariant.
        assert_ok!(MultiCollective::<Test>::swap_member(
            RuntimeOrigin::root(),
            CollectiveId::Alpha,
            alice,
            zara,
        ));
        assert_eq!(
            MultiCollective::<Test>::members_of(CollectiveId::Alpha),
            vec![charlie, dave, zara]
        );
    });
}

#[test]
fn swap_member_requires_origin() {
    TestState::build_and_execute(|| {
        let alice = U256::from(1);
        assert_ok!(MultiCollective::<Test>::add_member(
            RuntimeOrigin::root(),
            CollectiveId::Alpha,
            alice,
        ));

        assert_noop!(
            MultiCollective::<Test>::swap_member(
                RuntimeOrigin::signed(U256::from(999)),
                CollectiveId::Alpha,
                alice,
                U256::from(2),
            ),
            DispatchError::BadOrigin
        );

        assert_eq!(
            MultiCollective::<Test>::members_of(CollectiveId::Alpha),
            vec![alice]
        );
    });
}

#[test]
fn swap_member_fails_for_unknown_collective() {
    TestState::build_and_execute(|| {
        assert_noop!(
            MultiCollective::<Test>::swap_member(
                RuntimeOrigin::root(),
                CollectiveId::Unknown,
                U256::from(1),
                U256::from(2),
            ),
            Error::<Test>::CollectiveNotFound
        );

        assert!(multi_collective_events().is_empty());
    });
}

#[test]
fn swap_member_rejects_missing_remove() {
    TestState::build_and_execute(|| {
        assert_noop!(
            MultiCollective::<Test>::swap_member(
                RuntimeOrigin::root(),
                CollectiveId::Alpha,
                U256::from(1),
                U256::from(2),
            ),
            Error::<Test>::NotMember
        );

        assert!(multi_collective_events().is_empty());
    });
}

#[test]
fn swap_member_rejects_existing_add() {
    TestState::build_and_execute(|| {
        let alice = U256::from(1);
        let bob = U256::from(2);

        for who in [alice, bob] {
            assert_ok!(MultiCollective::<Test>::add_member(
                RuntimeOrigin::root(),
                CollectiveId::Alpha,
                who,
            ));
        }

        assert_noop!(
            MultiCollective::<Test>::swap_member(
                RuntimeOrigin::root(),
                CollectiveId::Alpha,
                alice,
                bob,
            ),
            Error::<Test>::AlreadyMember
        );

        // Both still present, in their original order.
        assert_eq!(
            MultiCollective::<Test>::members_of(CollectiveId::Alpha),
            vec![alice, bob]
        );
    });
}

#[test]
fn swap_member_rejects_self_swap() {
    TestState::build_and_execute(|| {
        let alice = U256::from(1);
        assert_ok!(MultiCollective::<Test>::add_member(
            RuntimeOrigin::root(),
            CollectiveId::Alpha,
            alice,
        ));

        // `remove` matches a member, so `NotMember` doesn't fire; the next
        // check (`!contains(add)`) rejects because add is already present
        // (it is `remove` itself). "Swap with self" is a no-op the pallet
        // refuses.
        assert_noop!(
            MultiCollective::<Test>::swap_member(
                RuntimeOrigin::root(),
                CollectiveId::Alpha,
                alice,
                alice,
            ),
            Error::<Test>::AlreadyMember
        );

        assert_eq!(
            MultiCollective::<Test>::members_of(CollectiveId::Alpha),
            vec![alice]
        );
    });
}

/// Beta has `min_members = 2, max_members = 3`. Swap is count-invariant
/// and skips both bounds checks, so it must succeed at either end.
/// Setup walks the collective from min to max via `add_member`, then
/// swaps once at each bound.
#[test]
fn swap_member_works_at_bounds() {
    TestState::build_and_execute(|| {
        let alice = U256::from(1);
        let bob = U256::from(2);
        let carol = U256::from(3);
        let dave = U256::from(4);
        let erin = U256::from(5);

        for who in [alice, bob] {
            assert_ok!(MultiCollective::<Test>::add_member(
                RuntimeOrigin::root(),
                CollectiveId::Beta,
                who,
            ));
        }

        // At min: swap alice for carol.
        assert_ok!(MultiCollective::<Test>::swap_member(
            RuntimeOrigin::root(),
            CollectiveId::Beta,
            alice,
            carol,
        ));
        assert_eq!(MultiCollective::<Test>::member_count(CollectiveId::Beta), 2);
        assert!(!MultiCollective::<Test>::is_member(
            CollectiveId::Beta,
            &alice
        ));
        assert!(MultiCollective::<Test>::is_member(
            CollectiveId::Beta,
            &carol
        ));

        // Grow to max, then at max: swap carol for dave.
        assert_ok!(MultiCollective::<Test>::add_member(
            RuntimeOrigin::root(),
            CollectiveId::Beta,
            dave,
        ));
        assert_eq!(MultiCollective::<Test>::member_count(CollectiveId::Beta), 3);

        assert_ok!(MultiCollective::<Test>::swap_member(
            RuntimeOrigin::root(),
            CollectiveId::Beta,
            carol,
            erin,
        ));
        assert_eq!(MultiCollective::<Test>::member_count(CollectiveId::Beta), 3);
        assert!(!MultiCollective::<Test>::is_member(
            CollectiveId::Beta,
            &carol
        ));
        assert!(MultiCollective::<Test>::is_member(
            CollectiveId::Beta,
            &erin
        ));
    });
}

#[test]
fn set_members_replaces_list() {
    TestState::build_and_execute(|| {
        let a = U256::from(1);
        let b = U256::from(2);
        let c = U256::from(3);
        let d = U256::from(4);
        let e = U256::from(5);

        for who in [a, b] {
            assert_ok!(MultiCollective::<Test>::add_member(
                RuntimeOrigin::root(),
                CollectiveId::Alpha,
                who,
            ));
        }

        assert_ok!(MultiCollective::<Test>::set_members(
            RuntimeOrigin::root(),
            CollectiveId::Alpha,
            vec![c, d, e],
        ));

        assert_eq!(
            MultiCollective::<Test>::members_of(CollectiveId::Alpha),
            vec![c, d, e]
        );
        assert!(!MultiCollective::<Test>::is_member(CollectiveId::Alpha, &a));
        assert!(!MultiCollective::<Test>::is_member(CollectiveId::Alpha, &b));

        assert_eq!(
            multi_collective_events().last(),
            Some(&CollectiveEvent::MembersSet {
                collective_id: CollectiveId::Alpha,
                outgoing: vec![a, b],
                incoming: vec![c, d, e],
            })
        );
    });
}

#[test]
fn set_members_handles_overlap() {
    TestState::build_and_execute(|| {
        let a = U256::from(1);
        let b = U256::from(2);
        let c = U256::from(3);
        let d = U256::from(4);

        for who in [a, b, c] {
            assert_ok!(MultiCollective::<Test>::add_member(
                RuntimeOrigin::root(),
                CollectiveId::Alpha,
                who,
            ));
        }

        // [b, c, d] overlaps with the old [a, b, c]: b and c stay, a goes out,
        // d comes in. Final storage reflects the new list verbatim.
        assert_ok!(MultiCollective::<Test>::set_members(
            RuntimeOrigin::root(),
            CollectiveId::Alpha,
            vec![b, c, d],
        ));

        assert_eq!(
            MultiCollective::<Test>::members_of(CollectiveId::Alpha),
            vec![b, c, d]
        );

        assert_eq!(
            multi_collective_events().last(),
            Some(&CollectiveEvent::MembersSet {
                collective_id: CollectiveId::Alpha,
                outgoing: vec![a],
                incoming: vec![d],
            })
        );
    });
}

#[test]
fn set_members_requires_origin() {
    TestState::build_and_execute(|| {
        assert_noop!(
            MultiCollective::<Test>::set_members(
                RuntimeOrigin::signed(U256::from(999)),
                CollectiveId::Alpha,
                vec![U256::from(1)],
            ),
            DispatchError::BadOrigin
        );

        assert!(MultiCollective::<Test>::members_of(CollectiveId::Alpha).is_empty());
        assert!(multi_collective_events().is_empty());
    });
}

#[test]
fn set_members_fails_for_unknown_collective() {
    TestState::build_and_execute(|| {
        assert_noop!(
            MultiCollective::<Test>::set_members(
                RuntimeOrigin::root(),
                CollectiveId::Unknown,
                vec![U256::from(1)],
            ),
            Error::<Test>::CollectiveNotFound
        );

        assert!(multi_collective_events().is_empty());
    });
}

#[test]
fn set_members_rejects_too_few() {
    TestState::build_and_execute(|| {
        // Beta declares min_members = 2.
        assert_noop!(
            MultiCollective::<Test>::set_members(
                RuntimeOrigin::root(),
                CollectiveId::Beta,
                vec![U256::from(1)],
            ),
            Error::<Test>::TooFewMembers
        );

        assert!(MultiCollective::<Test>::members_of(CollectiveId::Beta).is_empty());
        assert!(multi_collective_events().is_empty());
    });
}

#[test]
fn set_members_rejects_too_many_via_info() {
    TestState::build_and_execute(|| {
        // Beta declares max_members = Some(3); four accounts is one over.
        let list: Vec<U256> = (1..=4u32).map(U256::from).collect();
        assert_noop!(
            MultiCollective::<Test>::set_members(RuntimeOrigin::root(), CollectiveId::Beta, list,),
            Error::<Test>::TooManyMembers
        );

        assert!(MultiCollective::<Test>::members_of(CollectiveId::Beta).is_empty());
        assert!(multi_collective_events().is_empty());
    });
}

#[test]
fn set_members_rejects_too_many_via_storage() {
    TestState::build_and_execute(|| {
        // Gamma's info.max_members is None; only T::MaxMembers = 32 applies.
        // 33 accounts exceed the BoundedVec bound, caught by try_from.
        let list: Vec<U256> = (1..=33u32).map(U256::from).collect();
        assert_noop!(
            MultiCollective::<Test>::set_members(RuntimeOrigin::root(), CollectiveId::Gamma, list,),
            Error::<Test>::TooManyMembers
        );

        assert!(MultiCollective::<Test>::members_of(CollectiveId::Gamma).is_empty());
    });
}

#[test]
fn set_members_rejects_duplicates() {
    TestState::build_and_execute(|| {
        let a = U256::from(1);
        let b = U256::from(2);

        assert_noop!(
            MultiCollective::<Test>::set_members(
                RuntimeOrigin::root(),
                CollectiveId::Alpha,
                vec![a, b, a],
            ),
            Error::<Test>::DuplicateAccounts
        );

        assert!(MultiCollective::<Test>::members_of(CollectiveId::Alpha).is_empty());
    });
}

/// Setting a list identical to the current membership still emits a
/// `MembersSet` event; the pallet doesn't short-circuit no-op sets.
/// Pinned so downstream consumers know they must tolerate empty-diff calls.
#[test]
fn set_members_noop_still_fires_event() {
    TestState::build_and_execute(|| {
        let a = U256::from(1);
        let b = U256::from(2);

        for who in [a, b] {
            assert_ok!(MultiCollective::<Test>::add_member(
                RuntimeOrigin::root(),
                CollectiveId::Alpha,
                who,
            ));
        }

        assert_ok!(MultiCollective::<Test>::set_members(
            RuntimeOrigin::root(),
            CollectiveId::Alpha,
            vec![a, b],
        ));

        assert_eq!(
            MultiCollective::<Test>::members_of(CollectiveId::Alpha),
            vec![a, b]
        );

        assert_eq!(
            multi_collective_events().last(),
            Some(&CollectiveEvent::MembersSet {
                collective_id: CollectiveId::Alpha,
                incoming: vec![],
                outgoing: vec![],
            })
        );
    });
}

#[test]
fn on_initialize_no_rotation_when_term_duration_none() {
    TestState::build_and_execute(|| {
        // Alpha (td=None) and Gamma (td=None) must never appear in the log
        // regardless of how many blocks pass.
        run_to_block(300);

        let log = take_new_term_log();
        assert!(
            !log.contains(&CollectiveId::Alpha),
            "Alpha has term_duration = None; should never rotate"
        );
        assert!(
            !log.contains(&CollectiveId::Gamma),
            "Gamma has term_duration = None; should never rotate"
        );
    });
}

#[test]
fn on_initialize_no_rotation_between_boundaries() {
    TestState::build_and_execute(|| {
        // Earliest boundary is Delta's at block 50. Before that, nothing fires.
        run_to_block(49);
        assert!(take_new_term_log().is_empty());
    });
}

#[test]
fn on_initialize_fires_rotation_at_modulo_boundary() {
    TestState::build_and_execute(|| {
        // Delta (td=50) first fires at block 50. The "no rotation between
        // boundaries" property is covered by
        // `on_initialize_no_rotation_between_boundaries`.
        run_to_block(50);
        assert_eq!(take_new_term_log(), vec![CollectiveId::Delta]);
    });
}

#[test]
fn on_initialize_fires_all_matching_collectives() {
    TestState::build_and_execute(|| {
        // Advance through the first shared boundary at block 100. Delta fires
        // at 50, then both Beta and Delta fire at 100. Iteration order in
        // `TestCollectives` is [Alpha, Beta, Gamma, Delta], so within block
        // 100 the log gets Beta before Delta.
        run_to_block(100);

        assert_eq!(
            take_new_term_log(),
            vec![
                CollectiveId::Delta, // block 50
                CollectiveId::Beta,  // block 100
                CollectiveId::Delta, // block 100
            ]
        );

        // Next cadence: only Delta at 150, both again at 200.
        run_to_block(150);
        assert_eq!(take_new_term_log(), vec![CollectiveId::Delta]);

        run_to_block(200);
        assert_eq!(
            take_new_term_log(),
            vec![CollectiveId::Beta, CollectiveId::Delta]
        );
    });
}

#[test]
fn force_rotate_routes_through_on_new_term() {
    TestState::build_and_execute(|| {
        // Beta has term_duration = Some(100), so it's eligible.
        assert_ok!(MultiCollective::<Test>::force_rotate(
            RuntimeOrigin::root(),
            CollectiveId::Beta,
        ));
        assert_eq!(take_new_term_log(), vec![CollectiveId::Beta]);
    });
}

#[test]
fn force_rotate_requires_origin() {
    TestState::build_and_execute(|| {
        assert_noop!(
            MultiCollective::<Test>::force_rotate(
                RuntimeOrigin::signed(U256::from(1)),
                CollectiveId::Beta,
            ),
            DispatchError::BadOrigin,
        );
        assert!(take_new_term_log().is_empty());
    });
}

#[test]
fn force_rotate_rejects_non_rotating_collective() {
    TestState::build_and_execute(|| {
        // Alpha has `term_duration: None`.
        assert_noop!(
            MultiCollective::<Test>::force_rotate(RuntimeOrigin::root(), CollectiveId::Alpha,),
            Error::<Test>::CollectiveDoesNotRotate,
        );
        assert!(take_new_term_log().is_empty());
    });
}

#[test]
fn force_rotate_rejects_unknown_collective() {
    TestState::build_and_execute(|| {
        assert_noop!(
            MultiCollective::<Test>::force_rotate(RuntimeOrigin::root(), CollectiveId::Unknown,),
            Error::<Test>::CollectiveNotFound,
        );
        assert!(take_new_term_log().is_empty());
    });
}

#[test]
fn inspect_is_member_basic() {
    TestState::build_and_execute(|| {
        let alice = U256::from(1);
        let mallory = U256::from(999);

        // Empty collective: no membership.
        assert!(!MultiCollective::<Test>::is_member(
            CollectiveId::Alpha,
            &alice
        ));

        assert_ok!(MultiCollective::<Test>::add_member(
            RuntimeOrigin::root(),
            CollectiveId::Alpha,
            alice,
        ));

        assert!(MultiCollective::<Test>::is_member(
            CollectiveId::Alpha,
            &alice
        ));
        assert!(!MultiCollective::<Test>::is_member(
            CollectiveId::Alpha,
            &mallory
        ));
        // Membership is per-collective; alice isn't in Beta.
        assert!(!MultiCollective::<Test>::is_member(
            CollectiveId::Beta,
            &alice
        ));
    });
}

#[test]
fn inspect_member_count_matches_mutations() {
    TestState::build_and_execute(|| {
        let a = U256::from(1);
        let b = U256::from(2);
        let c = U256::from(3);
        let d = U256::from(4);

        assert_eq!(
            MultiCollective::<Test>::member_count(CollectiveId::Alpha),
            0
        );

        assert_ok!(MultiCollective::<Test>::add_member(
            RuntimeOrigin::root(),
            CollectiveId::Alpha,
            a,
        ));
        assert_eq!(
            MultiCollective::<Test>::member_count(CollectiveId::Alpha),
            1
        );

        assert_ok!(MultiCollective::<Test>::add_member(
            RuntimeOrigin::root(),
            CollectiveId::Alpha,
            b,
        ));
        assert_eq!(
            MultiCollective::<Test>::member_count(CollectiveId::Alpha),
            2
        );

        // Swap is count-invariant.
        assert_ok!(MultiCollective::<Test>::swap_member(
            RuntimeOrigin::root(),
            CollectiveId::Alpha,
            a,
            c,
        ));
        assert_eq!(
            MultiCollective::<Test>::member_count(CollectiveId::Alpha),
            2
        );

        // Remove decrements by one.
        assert_ok!(MultiCollective::<Test>::remove_member(
            RuntimeOrigin::root(),
            CollectiveId::Alpha,
            b,
        ));
        assert_eq!(
            MultiCollective::<Test>::member_count(CollectiveId::Alpha),
            1
        );

        // `set_members` replaces wholesale; count reflects the new list length.
        assert_ok!(MultiCollective::<Test>::set_members(
            RuntimeOrigin::root(),
            CollectiveId::Alpha,
            vec![a, b, c, d],
        ));
        assert_eq!(
            MultiCollective::<Test>::member_count(CollectiveId::Alpha),
            4
        );
    });
}

#[test]
fn inspect_of_unknown_collective_returns_empty() {
    TestState::build_and_execute(|| {
        // `Unknown` is not registered in TestCollectives::collectives().
        // `Members` storage uses ValueQuery and returns an empty BoundedVec by
        // default, so all three reads succeed without error or panic.
        assert_eq!(
            MultiCollective::<Test>::members_of(CollectiveId::Unknown),
            Vec::<U256>::new()
        );
        assert!(!MultiCollective::<Test>::is_member(
            CollectiveId::Unknown,
            &U256::from(1)
        ));
        assert_eq!(
            MultiCollective::<Test>::member_count(CollectiveId::Unknown),
            0
        );
    });
}

// `integrity_test_passes_on_valid_config` is implicit: the mock's
// auto-generated `__construct_runtime_integrity_test::runtime_integrity_tests`
// runs `integrity_test()` against the default `TestCollectives` on every
// `cargo test`. Listed in test output as `mock::...runtime_integrity_tests`.

fn bad_min_exceeds_storage() -> Vec<Collective<CollectiveId, u64, [u8; 32]>> {
    vec![Collective {
        id: CollectiveId::Alpha,
        info: CollectiveInfo {
            name: name_bytes(b"bad"),
            // T::MaxMembers = 32 in the mock; 100 exceeds storage capacity.
            min_members: 100,
            max_members: Some(200),
            term_duration: None,
        },
    }]
}

fn bad_max_exceeds_storage() -> Vec<Collective<CollectiveId, u64, [u8; 32]>> {
    vec![Collective {
        id: CollectiveId::Alpha,
        info: CollectiveInfo {
            name: name_bytes(b"bad"),
            min_members: 0,
            // T::MaxMembers = 32; max_members = 100 is declaratively larger.
            max_members: Some(100),
            term_duration: None,
        },
    }]
}

fn bad_min_exceeds_info_max() -> Vec<Collective<CollectiveId, u64, [u8; 32]>> {
    vec![Collective {
        id: CollectiveId::Alpha,
        info: CollectiveInfo {
            name: name_bytes(b"bad"),
            // min > max: the collective can never satisfy both.
            min_members: 5,
            max_members: Some(3),
            term_duration: None,
        },
    }]
}

fn bad_term_duration_zero() -> Vec<Collective<CollectiveId, u64, [u8; 32]>> {
    vec![Collective {
        id: CollectiveId::Alpha,
        info: CollectiveInfo {
            name: name_bytes(b"bad"),
            min_members: 0,
            max_members: Some(5),
            // Some(0) silently disables rotations; integrity_test rejects it.
            term_duration: Some(0),
        },
    }]
}

#[test]
#[should_panic(expected = "min_members (100) exceeds T::MaxMembers (32)")]
fn integrity_test_panics_on_min_exceeds_storage_max() {
    with_collectives_override(bad_min_exceeds_storage, || {
        <MultiCollective<Test> as Hooks<u64>>::integrity_test();
    });
}

#[test]
#[should_panic(expected = "max_members (100) exceeds T::MaxMembers (32)")]
fn integrity_test_panics_on_max_exceeds_storage_max() {
    with_collectives_override(bad_max_exceeds_storage, || {
        <MultiCollective<Test> as Hooks<u64>>::integrity_test();
    });
}

#[test]
#[should_panic(expected = "min_members (5) exceeds max_members (3)")]
fn integrity_test_panics_on_min_exceeds_info_max() {
    with_collectives_override(bad_min_exceeds_info_max, || {
        <MultiCollective<Test> as Hooks<u64>>::integrity_test();
    });
}

#[test]
#[should_panic(expected = "silently disables rotations")]
fn integrity_test_panics_on_term_duration_zero() {
    with_collectives_override(bad_term_duration_zero, || {
        <MultiCollective<Test> as Hooks<u64>>::integrity_test();
    });
}

// `OnMembersChanged` payload tests. The pallet's events show what changed
// in storage but not what was passed to the hook, so an argument-order
// regression (e.g. swapping `incoming` and `outgoing`) would not be
// caught by the event assertions alone.

#[test]
fn on_members_changed_payload_for_add_member() {
    TestState::build_and_execute(|| {
        let alice = U256::from(1);
        assert_ok!(MultiCollective::<Test>::add_member(
            RuntimeOrigin::root(),
            CollectiveId::Alpha,
            alice,
        ));
        assert_eq!(
            take_members_changed_log(),
            vec![MembersChangedCall {
                collective_id: CollectiveId::Alpha,
                incoming: vec![alice],
                outgoing: vec![],
            }]
        );
    });
}

#[test]
fn on_members_changed_payload_for_remove_member() {
    TestState::build_and_execute(|| {
        let alice = U256::from(1);
        let bob = U256::from(2);
        for who in [alice, bob] {
            assert_ok!(MultiCollective::<Test>::add_member(
                RuntimeOrigin::root(),
                CollectiveId::Alpha,
                who,
            ));
        }
        let _ = take_members_changed_log();

        assert_ok!(MultiCollective::<Test>::remove_member(
            RuntimeOrigin::root(),
            CollectiveId::Alpha,
            bob,
        ));
        assert_eq!(
            take_members_changed_log(),
            vec![MembersChangedCall {
                collective_id: CollectiveId::Alpha,
                incoming: vec![],
                outgoing: vec![bob],
            }]
        );
    });
}

#[test]
fn on_members_changed_payload_for_swap_member() {
    TestState::build_and_execute(|| {
        let alice = U256::from(1);
        let bob = U256::from(2);
        for who in [alice, bob] {
            assert_ok!(MultiCollective::<Test>::add_member(
                RuntimeOrigin::root(),
                CollectiveId::Alpha,
                who,
            ));
        }
        let _ = take_members_changed_log();

        let carol = U256::from(3);
        assert_ok!(MultiCollective::<Test>::swap_member(
            RuntimeOrigin::root(),
            CollectiveId::Alpha,
            alice,
            carol,
        ));
        assert_eq!(
            take_members_changed_log(),
            vec![MembersChangedCall {
                collective_id: CollectiveId::Alpha,
                incoming: vec![carol],
                outgoing: vec![alice],
            }]
        );
    });
}

#[test]
fn on_members_changed_payload_for_set_members() {
    TestState::build_and_execute(|| {
        let a = U256::from(1);
        let b = U256::from(2);
        let c = U256::from(3);
        let d = U256::from(4);
        for who in [a, b, c] {
            assert_ok!(MultiCollective::<Test>::add_member(
                RuntimeOrigin::root(),
                CollectiveId::Alpha,
                who,
            ));
        }
        let _ = take_members_changed_log();

        assert_ok!(MultiCollective::<Test>::set_members(
            RuntimeOrigin::root(),
            CollectiveId::Alpha,
            vec![b, c, d],
        ));
        assert_eq!(
            take_members_changed_log(),
            vec![MembersChangedCall {
                collective_id: CollectiveId::Alpha,
                incoming: vec![d],
                outgoing: vec![a],
            }]
        );
    });
}

// `do_try_state` direct tests. The extrinsics maintain the invariants by
// construction, so corrupting `Members` storage manually is the only way
// to exercise each failure branch.

fn write_raw_members(id: CollectiveId, members: Vec<U256>) {
    let bounded = BoundedVec::try_from(members).expect("test fixture must fit MaxMembers");
    crate::pallet::Members::<Test>::insert(id, bounded);
}

#[test]
fn try_state_passes_on_valid_storage() {
    TestState::build_and_execute(|| {
        for who in [U256::from(1), U256::from(2)] {
            assert_ok!(MultiCollective::<Test>::add_member(
                RuntimeOrigin::root(),
                CollectiveId::Alpha,
                who,
            ));
        }
        assert!(MultiCollective::<Test>::do_try_state().is_ok());
    });
}

#[test]
fn try_state_rejects_unsorted_storage() {
    TestState::build_and_execute(|| {
        write_raw_members(CollectiveId::Alpha, vec![U256::from(2), U256::from(1)]);
        assert!(MultiCollective::<Test>::do_try_state().is_err());
    });
}

#[test]
fn try_state_rejects_orphan_collective_row() {
    TestState::build_and_execute(|| {
        // `Unknown` is reachable via the storage map's `Blake2_128Concat`
        // hash but is not registered in `TestCollectives::collectives()`.
        write_raw_members(CollectiveId::Unknown, vec![U256::from(1)]);
        assert!(MultiCollective::<Test>::do_try_state().is_err());
    });
}

#[test]
fn try_state_rejects_count_exceeding_info_max() {
    TestState::build_and_execute(|| {
        // Beta declares max_members = 3; four entries fit the BoundedVec
        // bound (T::MaxMembers = 32) but violate the per-collective cap.
        let four: Vec<U256> = (1..=4u32).map(U256::from).collect();
        write_raw_members(CollectiveId::Beta, four);
        assert!(MultiCollective::<Test>::do_try_state().is_err());
    });
}

/// `set_members` sorts its input before writing. Without this step,
/// downstream `binary_search` and `compute_members_diff_sorted` calls
/// would silently observe an unsorted storage entry; pinning the sort
/// here guards against a regression that drops the `sorted.sort()` call.
#[test]
fn set_members_sorts_input() {
    TestState::build_and_execute(|| {
        let a = U256::from(1);
        let b = U256::from(2);
        let c = U256::from(3);

        assert_ok!(MultiCollective::<Test>::set_members(
            RuntimeOrigin::root(),
            CollectiveId::Alpha,
            vec![c, a, b],
        ));

        assert_eq!(
            MultiCollective::<Test>::members_of(CollectiveId::Alpha),
            vec![a, b, c]
        );
    });
}

/// `force_rotate` returns `Some(actual_weight)` equal to
/// `WeightInfo::force_rotate() + OnNewTerm::on_new_term(...)`. The mock's
/// `WeightInfo` is `()`, whose generated impl reports the pallet's base
/// dispatch cost, so the post-info weight should include that static cost
/// plus the hook's reported cost.
#[test]
fn force_rotate_returns_post_info_weight() {
    TestState::build_and_execute(|| {
        let hook_weight = Weight::from_parts(123_456, 0);
        set_new_term_weight(hook_weight);

        let post = MultiCollective::<Test>::force_rotate(RuntimeOrigin::root(), CollectiveId::Beta)
            .expect("force_rotate succeeds for Beta");

        assert_eq!(
            post.actual_weight,
            Some(
                <<Test as crate::Config>::WeightInfo as crate::WeightInfo>::force_rotate()
                    .saturating_add(hook_weight)
            )
        );
    });
}

/// The pallet ships a tuple impl of `OnNewTerm` so a runtime can fan a
/// rotation out to multiple handlers. The mock wires a single impl, so
/// without this test the tuple expansion is not exercised by `cargo test`.
#[test]
fn on_new_term_tuple_impl_dispatches_to_each_member() {
    TestState::build_and_execute(|| {
        set_new_term_weight(Weight::from_parts(7, 0));

        let combined = <(TestOnNewTerm, TestOnNewTerm) as OnNewTerm<CollectiveId>>::on_new_term(
            CollectiveId::Beta,
        );

        assert_eq!(combined, Weight::from_parts(14, 0));
        assert_eq!(
            take_new_term_log(),
            vec![CollectiveId::Beta, CollectiveId::Beta]
        );

        let weight = <(TestOnNewTerm, TestOnNewTerm) as OnNewTerm<CollectiveId>>::weight();
        assert_eq!(weight, Weight::from_parts(14, 0));
    });
}

#[test]
fn do_add_member_inserts_and_emits_event() {
    TestState::build_and_execute(|| {
        let who = U256::from(7);

        assert_ok!(MultiCollective::<Test>::do_add_member(
            CollectiveId::Alpha,
            who,
        ));

        assert_eq!(
            MultiCollective::<Test>::members_of(CollectiveId::Alpha),
            vec![who]
        );
        assert_eq!(
            take_members_changed_log(),
            vec![MembersChangedCall {
                collective_id: CollectiveId::Alpha,
                incoming: vec![who],
                outgoing: vec![],
            }]
        );
        assert_eq!(
            multi_collective_events().last().expect("event emitted"),
            &CollectiveEvent::MemberAdded {
                collective_id: CollectiveId::Alpha,
                who,
            },
        );
    });
}

#[test]
fn do_add_member_errors_on_already_member() {
    TestState::build_and_execute(|| {
        let who = U256::from(7);
        assert_ok!(MultiCollective::<Test>::do_add_member(
            CollectiveId::Alpha,
            who,
        ));
        assert!(matches!(
            MultiCollective::<Test>::do_add_member(CollectiveId::Alpha, who),
            Err(Error::<Test>::AlreadyMember),
        ));
    });
}

#[test]
fn do_add_member_errors_on_unknown_collective() {
    TestState::build_and_execute(|| {
        assert!(matches!(
            MultiCollective::<Test>::do_add_member(CollectiveId::Unknown, U256::from(1)),
            Err(Error::<Test>::CollectiveNotFound),
        ));
    });
}

#[test]
fn do_add_member_errors_when_max_members_reached() {
    TestState::build_and_execute(|| {
        // Alpha caps at 5 members.
        for i in 0..5u32 {
            assert_ok!(MultiCollective::<Test>::do_add_member(
                CollectiveId::Alpha,
                U256::from(i),
            ));
        }
        assert!(matches!(
            MultiCollective::<Test>::do_add_member(CollectiveId::Alpha, U256::from(99)),
            Err(Error::<Test>::TooManyMembers),
        ));
    });
}

#[test]
fn do_remove_member_removes_and_emits_event() {
    TestState::build_and_execute(|| {
        let who = U256::from(7);
        assert_ok!(MultiCollective::<Test>::do_add_member(
            CollectiveId::Alpha,
            who,
        ));
        let _ = take_members_changed_log();

        assert_ok!(MultiCollective::<Test>::do_remove_member(
            CollectiveId::Alpha,
            who,
        ));

        assert!(MultiCollective::<Test>::members_of(CollectiveId::Alpha).is_empty());
        assert_eq!(
            take_members_changed_log(),
            vec![MembersChangedCall {
                collective_id: CollectiveId::Alpha,
                incoming: vec![],
                outgoing: vec![who],
            }]
        );
        assert_eq!(
            multi_collective_events().last().expect("event emitted"),
            &CollectiveEvent::MemberRemoved {
                collective_id: CollectiveId::Alpha,
                who,
            },
        );
    });
}

#[test]
fn do_remove_member_errors_on_non_member() {
    TestState::build_and_execute(|| {
        assert!(matches!(
            MultiCollective::<Test>::do_remove_member(CollectiveId::Alpha, U256::from(7)),
            Err(Error::<Test>::NotMember),
        ));
    });
}

#[test]
fn do_remove_member_respects_min_members_floor() {
    TestState::build_and_execute(|| {
        let a = U256::from(1);
        let b = U256::from(2);
        assert_ok!(MultiCollective::<Test>::set_members(
            RuntimeOrigin::root(),
            CollectiveId::Beta,
            vec![a, b],
        ));

        // Beta has min_members = 2; dropping below the floor must error.
        assert!(matches!(
            MultiCollective::<Test>::do_remove_member(CollectiveId::Beta, a),
            Err(Error::<Test>::TooFewMembers),
        ));
    });
}

#[test]
fn do_remove_member_errors_on_unknown_collective() {
    TestState::build_and_execute(|| {
        assert!(matches!(
            MultiCollective::<Test>::do_remove_member(CollectiveId::Unknown, U256::from(1)),
            Err(Error::<Test>::CollectiveNotFound),
        ));
    });
}
