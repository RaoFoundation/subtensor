//! Tests for commitments pallet: space limit.

use super::*;

#[test]
fn tempo_based_space_limit_accumulates_in_same_window() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        let who = 100;
        let space_limit = 150;
        MaxSpace::<Test>::set(space_limit);
        System::<Test>::set_block_number(0);

        // A single commitment that uses some space, e.g. 30 bytes:
        let data = vec![0u8; 30];
        let info = Box::new(CommitmentInfo {
            fields: BoundedVec::try_from(vec![Data::Raw(
                data.try_into().expect("Data up to 128 bytes OK"),
            )])
            .expect("1 field is <= MaxFields"),
        });

        // 2) First call => usage=0 => usage=30 after. OK.
        assert_ok!(Pallet::<Test>::set_commitment(
            RuntimeOrigin::signed(who),
            netuid,
            info.clone(),
        ));

        // 3) Second call => tries another 30 bytes in the SAME block => total=60 => exceeds 50 => should fail.
        assert_noop!(
            Pallet::<Test>::set_commitment(RuntimeOrigin::signed(who), netuid, info.clone()),
            Error::<Test>::SpaceLimitExceeded
        );
    });
}

#[test]
fn tempo_based_space_limit_resets_after_tempo() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(2);
        let who = 101;

        MaxSpace::<Test>::set(250);
        System::<Test>::set_block_number(1);

        let commit_small = Box::new(CommitmentInfo {
            fields: BoundedVec::try_from(vec![Data::Raw(
                vec![0u8; 20].try_into().expect("expected ok"),
            )])
            .expect("expected ok"),
        });

        assert_ok!(Pallet::<Test>::set_commitment(
            RuntimeOrigin::signed(who),
            netuid,
            commit_small.clone()
        ));

        assert_ok!(Pallet::<Test>::set_commitment(
            RuntimeOrigin::signed(who),
            netuid,
            commit_small.clone()
        ));

        assert_noop!(
            Pallet::<Test>::set_commitment(
                RuntimeOrigin::signed(who),
                netuid,
                commit_small.clone()
            ),
            Error::<Test>::SpaceLimitExceeded
        );

        System::<Test>::set_block_number(200);

        assert_noop!(
            Pallet::<Test>::set_commitment(
                RuntimeOrigin::signed(who),
                netuid,
                commit_small.clone()
            ),
            Error::<Test>::SpaceLimitExceeded
        );

        System::<Test>::set_block_number(360);

        assert_ok!(Pallet::<Test>::set_commitment(
            RuntimeOrigin::signed(who),
            netuid,
            commit_small
        ));
    });
}

#[test]
fn tempo_based_space_limit_does_not_affect_different_netuid() {
    new_test_ext().execute_with(|| {
        let netuid_a = NetUid::from(10);
        let netuid_b = NetUid::from(20);
        let who = 111;
        let space_limit = 199;
        MaxSpace::<Test>::set(space_limit);

        let commit_large = Box::new(CommitmentInfo {
            fields: BoundedVec::try_from(vec![Data::Raw(
                vec![0u8; 40].try_into().expect("expected ok"),
            )])
            .expect("expected ok"),
        });
        let commit_small = Box::new(CommitmentInfo {
            fields: BoundedVec::try_from(vec![Data::Raw(
                vec![0u8; 20].try_into().expect("expected ok"),
            )])
            .expect("expected ok"),
        });

        assert_ok!(Pallet::<Test>::set_commitment(
            RuntimeOrigin::signed(who),
            netuid_a,
            commit_large.clone()
        ));

        assert_noop!(
            Pallet::<Test>::set_commitment(
                RuntimeOrigin::signed(who),
                netuid_a,
                commit_small.clone()
            ),
            Error::<Test>::SpaceLimitExceeded
        );

        assert_ok!(Pallet::<Test>::set_commitment(
            RuntimeOrigin::signed(who),
            netuid_b,
            commit_large
        ));

        assert_noop!(
            Pallet::<Test>::set_commitment(RuntimeOrigin::signed(who), netuid_b, commit_small),
            Error::<Test>::SpaceLimitExceeded
        );
    });
}

#[test]
fn tempo_based_space_limit_does_not_affect_different_user() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(10);
        let user1 = 123;
        let user2 = 456;
        let space_limit = 199;
        MaxSpace::<Test>::set(space_limit);

        let commit_large = Box::new(CommitmentInfo {
            fields: BoundedVec::try_from(vec![Data::Raw(
                vec![0u8; 40].try_into().expect("expected ok"),
            )])
            .expect("expected ok"),
        });
        let commit_small = Box::new(CommitmentInfo {
            fields: BoundedVec::try_from(vec![Data::Raw(
                vec![0u8; 20].try_into().expect("expected ok"),
            )])
            .expect("expected ok"),
        });

        assert_ok!(Pallet::<Test>::set_commitment(
            RuntimeOrigin::signed(user1),
            netuid,
            commit_large.clone()
        ));

        assert_noop!(
            Pallet::<Test>::set_commitment(
                RuntimeOrigin::signed(user1),
                netuid,
                commit_small.clone()
            ),
            Error::<Test>::SpaceLimitExceeded
        );

        assert_ok!(Pallet::<Test>::set_commitment(
            RuntimeOrigin::signed(user2),
            netuid,
            commit_large
        ));

        assert_noop!(
            Pallet::<Test>::set_commitment(RuntimeOrigin::signed(user2), netuid, commit_small),
            Error::<Test>::SpaceLimitExceeded
        );
    });
}

#[test]
fn tempo_based_space_limit_sudo_set_max_space() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(3);
        let who = 15;
        MaxSpace::<Test>::set(100);

        System::<Test>::set_block_number(1);
        let commit_25 = Box::new(CommitmentInfo {
            fields: BoundedVec::try_from(vec![Data::Raw(
                vec![0u8; 25].try_into().expect("expected ok"),
            )])
            .expect("expected ok"),
        });

        assert_ok!(Pallet::<Test>::set_commitment(
            RuntimeOrigin::signed(who),
            netuid,
            commit_25.clone()
        ));
        assert_noop!(
            Pallet::<Test>::set_commitment(RuntimeOrigin::signed(who), netuid, commit_25.clone()),
            Error::<Test>::SpaceLimitExceeded
        );

        assert_ok!(Pallet::<Test>::set_max_space(RuntimeOrigin::root(), 300));

        assert_ok!(Pallet::<Test>::set_commitment(
            RuntimeOrigin::signed(who),
            netuid,
            commit_25
        ));
    });
}
