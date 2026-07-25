//! Tests for commitments pallet: timelocked index.

use super::*;

#[test]
fn test_index_lifecycle_no_timelocks_updates_in_out() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(100);
        let who = 999;

        //
        // A) Create a commitment with **no** timelocks => shouldn't be in index
        //
        let no_tl_fields: BoundedVec<Data, <Test as Config>::MaxFields> =
            BoundedVec::try_from(vec![]).expect("Empty is ok");
        let info_no_tl = CommitmentInfo {
            fields: no_tl_fields,
        };
        assert_ok!(Pallet::<Test>::set_commitment(
            RuntimeOrigin::signed(who),
            netuid,
            Box::new(info_no_tl)
        ));
        assert!(
            !TimelockedIndex::<Test>::get().contains(&(netuid, who)),
            "User with no timelocks must not appear in index"
        );

        //
        // B) Update the commitment to have a timelock => enters index
        //
        let tl_fields: BoundedVec<_, <Test as Config>::MaxFields> =
            BoundedVec::try_from(vec![Data::TimelockEncrypted {
                encrypted: Default::default(),
                reveal_round: 1234,
            }])
            .expect("Expected success");
        let info_with_tl = CommitmentInfo { fields: tl_fields };
        assert_ok!(Pallet::<Test>::set_commitment(
            RuntimeOrigin::signed(who),
            netuid,
            Box::new(info_with_tl)
        ));
        assert!(
            TimelockedIndex::<Test>::get().contains(&(netuid, who)),
            "User must appear in index after adding a timelock"
        );

        //
        // C) Remove the timelock => leaves index
        //
        let back_to_no_tl: BoundedVec<_, <Test as Config>::MaxFields> =
            BoundedVec::try_from(vec![]).expect("Expected success");
        let info_remove_tl = CommitmentInfo {
            fields: back_to_no_tl,
        };
        assert_ok!(Pallet::<Test>::set_commitment(
            RuntimeOrigin::signed(who),
            netuid,
            Box::new(info_remove_tl)
        ));

        assert!(
            !TimelockedIndex::<Test>::get().contains(&(netuid, who)),
            "User must be removed from index after losing all timelocks"
        );
    });
}

#[test]
fn timelocked_index_complex_scenario_works() {
    new_test_ext().execute_with(|| {
        System::<Test>::set_block_number(1);

        let netuid = NetUid::from(42);
        let user_a = 1000;
        let user_b = 2000;
        let user_c = 3000;

        let make_timelock_data = |plaintext: &[u8], round: u64| {
            let inner = CommitmentInfo::<TestMaxFields> {
                fields: BoundedVec::try_from(vec![Data::Raw(
                    plaintext.to_vec().try_into().expect("<=128 bytes"),
                )])
                .expect("1 field is fine"),
            };
            let ct = produce_ciphertext(&inner.encode(), round);
            Data::TimelockEncrypted {
                encrypted: ct,
                reveal_round: round,
            }
        };

        let make_raw_data =
            |payload: &[u8]| Data::Raw(payload.to_vec().try_into().expect("expected to not panic"));

        // ----------------------------------------------------
        // (1) USER A => no timelocks => NOT in index
        // ----------------------------------------------------
        let info_a1 = CommitmentInfo::<TestMaxFields> {
            fields: BoundedVec::try_from(vec![make_raw_data(b"A-regular")])
                .expect("1 field is fine"),
        };
        assert_ok!(Pallet::<Test>::set_commitment(
            RuntimeOrigin::signed(user_a),
            netuid,
            Box::new(info_a1),
        ));
        assert!(
            !TimelockedIndex::<Test>::get().contains(&(netuid, user_a)),
            "A has no timelocks => not in TimelockedIndex"
        );

        // ----------------------------------------------------
        // (2) USER B => Single TLE => BUT USE round=2000!
        //     => B is in index
        // ----------------------------------------------------
        let b_timelock_1 = make_timelock_data(b"B first TLE", 2000);
        let info_b1 = CommitmentInfo::<TestMaxFields> {
            fields: BoundedVec::try_from(vec![b_timelock_1]).expect("Single TLE is fine"),
        };
        assert_ok!(Pallet::<Test>::set_commitment(
            RuntimeOrigin::signed(user_b),
            netuid,
            Box::new(info_b1),
        ));
        let idx = TimelockedIndex::<Test>::get();
        assert!(!idx.contains(&(netuid, user_a)), "A not in index");
        assert!(idx.contains(&(netuid, user_b)), "B in index (has TLE)");

        // ----------------------------------------------------
        // (3) USER A => 2 timelocks: round=1000 & round=2000
        //     => A is in index
        // ----------------------------------------------------
        let a_timelock_1 = make_timelock_data(b"A TLE #1", 1000);
        let a_timelock_2 = make_timelock_data(b"A TLE #2", 2000);
        let info_a2 = CommitmentInfo::<TestMaxFields> {
            fields: BoundedVec::try_from(vec![a_timelock_1, a_timelock_2])
                .expect("2 TLE fields OK"),
        };
        assert_ok!(Pallet::<Test>::set_commitment(
            RuntimeOrigin::signed(user_a),
            netuid,
            Box::new(info_a2),
        ));

        let idx = TimelockedIndex::<Test>::get();
        assert!(idx.contains(&(netuid, user_a)), "A in index");
        assert!(idx.contains(&(netuid, user_b)), "B still in index");

        // ----------------------------------------------------
        // (4) USER B => remove all timelocks => B out of index
        // ----------------------------------------------------
        let info_b2 = CommitmentInfo::<TestMaxFields> {
            fields: BoundedVec::try_from(vec![make_raw_data(b"B back to raw")])
                .expect("no TLE => B out"),
        };
        assert_ok!(Pallet::<Test>::set_commitment(
            RuntimeOrigin::signed(user_b),
            netuid,
            Box::new(info_b2),
        ));
        let idx = TimelockedIndex::<Test>::get();
        assert!(idx.contains(&(netuid, user_a)), "A remains");
        assert!(
            !idx.contains(&(netuid, user_b)),
            "B removed after losing TLEs"
        );

        // ----------------------------------------------------
        // (5) USER B => re-add TLE => round=2000 => back in index
        // ----------------------------------------------------
        let b_timelock_2 = make_timelock_data(b"B TLE #2", 2000);
        let info_b3 = CommitmentInfo::<TestMaxFields> {
            fields: BoundedVec::try_from(vec![b_timelock_2]).expect("expected to not panic"),
        };
        assert_ok!(Pallet::<Test>::set_commitment(
            RuntimeOrigin::signed(user_b),
            netuid,
            Box::new(info_b3),
        ));
        let idx = TimelockedIndex::<Test>::get();
        assert!(idx.contains(&(netuid, user_a)), "A in index");
        assert!(idx.contains(&(netuid, user_b)), "B back in index");

        // ----------------------------------------------------
        // (6) USER C => sets 1 TLE => round=2000 => in index
        // ----------------------------------------------------
        let c_timelock_1 = make_timelock_data(b"C TLE #1", 2000);
        let info_c1 = CommitmentInfo::<TestMaxFields> {
            fields: BoundedVec::try_from(vec![c_timelock_1]).expect("expected to not panic"),
        };
        assert_ok!(Pallet::<Test>::set_commitment(
            RuntimeOrigin::signed(user_c),
            netuid,
            Box::new(info_c1),
        ));
        let idx = TimelockedIndex::<Test>::get();
        assert!(idx.contains(&(netuid, user_a)), "A");
        assert!(idx.contains(&(netuid, user_b)), "B");
        assert!(idx.contains(&(netuid, user_c)), "C");

        // ----------------------------------------------------
        // (7) Partial reveal for round=1000 => affects only A
        //     because B & C have round=2000
        // ----------------------------------------------------
        let drand_sig_1000 =
            hex::decode(DRAND_QUICKNET_SIG_HEX).expect("decode signature for round=1000");
        insert_drand_pulse(1000, &drand_sig_1000);

        System::<Test>::set_block_number(10);
        assert_ok!(Pallet::<Test>::reveal_timelocked_commitments());

        // After revealing round=1000:
        // - A: Loses TLE #1 (1000), still has TLE #2 (2000) => remains in index
        // - B: referencing 2000 => unaffected => remains
        // - C: referencing 2000 => remains
        let idx = TimelockedIndex::<Test>::get();
        assert!(
            idx.contains(&(netuid, user_a)),
            "A has leftover round=2000 => remains in index"
        );
        assert!(idx.contains(&(netuid, user_b)), "B unaffected");
        assert!(idx.contains(&(netuid, user_c)), "C unaffected");

        // ----------------------------------------------------
        // (8) Reveal round=2000 => fully remove A, B, and C
        // ----------------------------------------------------
        let drand_sig_2000 =
            hex::decode(DRAND_QUICKNET_SIG_2000_HEX).expect("decode signature for round=2000");
        insert_drand_pulse(2000, &drand_sig_2000);

        System::<Test>::set_block_number(11);
        assert_ok!(Pallet::<Test>::reveal_timelocked_commitments());

        // Now:
        // - A's final TLE (#2 at 2000) is removed => A out
        // - B had 2000 => out
        // - C had 2000 => out
        let idx = TimelockedIndex::<Test>::get();
        assert!(
            !idx.contains(&(netuid, user_a)),
            "A removed after 2000 reveal"
        );
        assert!(
            !idx.contains(&(netuid, user_b)),
            "B removed after 2000 reveal"
        );
        assert!(
            !idx.contains(&(netuid, user_c)),
            "C removed after 2000 reveal"
        );

        assert_eq!(idx.len(), 0, "All users revealed => index is empty");
    });
}
