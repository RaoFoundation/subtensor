//! Commitments that mix TimelockEncrypted fields with Raw / other Data variants.

use super::*;

#[allow(clippy::indexing_slicing)]
#[test]
fn multiple_timelocked_commitments_reveal_works() {
    new_test_ext().execute_with(|| {
        // -------------------------------------------
        // 1) Set up initial block number and user
        // -------------------------------------------
        let cur_block = 5u64.into();
        System::<Test>::set_block_number(cur_block);

        let who = 123;
        let netuid = NetUid::from(999);

        // -------------------------------------------
        // 2) Create multiple TLE fields referencing
        //    two known valid Drand rounds: 1000, 2000
        // -------------------------------------------

        let round_1000 = 1000;
        let round_2000 = 2000;

        // 2.a) TLE #1 => round=1000
        let tle_1_plaintext = b"Timelock #1 => round=1000";
        let ciphertext_1 = produce_ciphertext(tle_1_plaintext, round_1000);
        let tle_1 = Data::TimelockEncrypted {
            encrypted: ciphertext_1,
            reveal_round: round_1000,
        };

        // 2.b) TLE #2 => round=1000
        let tle_2_plaintext = b"Timelock #2 => round=1000";
        let ciphertext_2 = produce_ciphertext(tle_2_plaintext, round_1000);
        let tle_2 = Data::TimelockEncrypted {
            encrypted: ciphertext_2,
            reveal_round: round_1000,
        };

        // 2.c) TLE #3 => round=2000
        let tle_3_plaintext = b"Timelock #3 => round=2000";
        let ciphertext_3 = produce_ciphertext(tle_3_plaintext, round_2000);
        let tle_3 = Data::TimelockEncrypted {
            encrypted: ciphertext_3,
            reveal_round: round_2000,
        };

        // 2.d) TLE #4 => round=2000
        let tle_4_plaintext = b"Timelock #4 => round=2000";
        let ciphertext_4 = produce_ciphertext(tle_4_plaintext, round_2000);
        let tle_4 = Data::TimelockEncrypted {
            encrypted: ciphertext_4,
            reveal_round: round_2000,
        };

        // -------------------------------------------
        // 3) Insert all TLEs in a single CommitmentInfo
        // -------------------------------------------
        let fields = vec![tle_1, tle_2, tle_3, tle_4];
        let fields_bounded = BoundedVec::try_from(fields).expect("Must not exceed MaxFields");
        let info = CommitmentInfo {
            fields: fields_bounded,
        };

        // -------------------------------------------
        // 4) set_commitment => user is now in TimelockedIndex
        // -------------------------------------------
        assert_ok!(Pallet::<Test>::set_commitment(
            RuntimeOrigin::signed(who),
            netuid,
            Box::new(info)
        ));
        assert!(
            TimelockedIndex::<Test>::get().contains(&(netuid, who)),
            "User must appear in TimelockedIndex since they have TLE fields"
        );

        // Confirm the stored fields are as expected
        let stored = CommitmentOf::<Test>::get(netuid, who).expect("Should be stored");
        assert_eq!(
            stored.info.fields.len(),
            4,
            "All 4 timelock fields must be stored"
        );

        // -------------------------------------------
        // 5) Insert valid Drand pulse => round=1000
        // -------------------------------------------
        let drand_sig_1000 = hex::decode(DRAND_QUICKNET_SIG_HEX).expect("decode signature");
        insert_drand_pulse(round_1000, &drand_sig_1000);

        // Reveal at block=6 => should remove TLE #1 and TLE #2, leaving TLE #3, #4
        System::<Test>::set_block_number(6u64.into());
        assert_ok!(Pallet::<Test>::reveal_timelocked_commitments());

        // Check leftover => TLE #3, TLE #4 remain
        let leftover_after_1000 = CommitmentOf::<Test>::get(netuid, who).expect("Must exist");
        assert_eq!(
            leftover_after_1000.info.fields.len(),
            2,
            "After revealing round=1000, 2 timelocks remain (#3, #4)"
        );

        // Check partial reveals => TLE #1 & #2 in revealed storage
        let revealed_1000 = RevealedCommitments::<Test>::get(netuid, who)
            .expect("Should have partial reveals");
        assert_eq!(
            revealed_1000.len(),
            2,
            "We revealed exactly 2 items at round=1000"
        );
        {
            let (bytes_a, _) = &revealed_1000[0];
            let (bytes_b, _) = &revealed_1000[1];
            let txt_a = sp_std::str::from_utf8(bytes_a).expect("utf-8 expected");
            let txt_b = sp_std::str::from_utf8(bytes_b).expect("utf-8 expected");
            assert!(
                txt_a.contains("Timelock #1") || txt_a.contains("Timelock #2"),
                "Revealed #1 or #2"
            );
            assert!(
                txt_b.contains("Timelock #1") || txt_b.contains("Timelock #2"),
                "Revealed #1 or #2"
            );
        }

        assert!(
            TimelockedIndex::<Test>::get().contains(&(netuid, who)),
            "TLE left"
        );

        // -------------------------------------------
        // 6) Insert valid Drand pulse => round=2000
        // -------------------------------------------
        let drand_sig_2000_hex =
            "b6cb8f482a0b15d45936a4c4ea08e98a087e71787caee3f4d07a8a9843b1bc5423c6b3c22f446488b3137eaca799c77e";
        let drand_sig_2000 = hex::decode(drand_sig_2000_hex).expect("decode signature");
        insert_drand_pulse(round_2000, &drand_sig_2000);

        // Reveal at block=7 => should remove TLE #3 and TLE #4
        System::<Test>::set_block_number(7u64.into());
        assert_ok!(Pallet::<Test>::reveal_timelocked_commitments());

        // After revealing these last two timelocks => leftover is none
        let leftover_after_2000 = CommitmentOf::<Test>::get(netuid, who);
        assert!(
            leftover_after_2000.is_none(),
            "All timelocks revealed => leftover none => entry removed"
        );

        // Because the user has no timelocks left => removed from TimelockedIndex
        assert!(
            !TimelockedIndex::<Test>::get().contains(&(netuid, who)),
            "No TLE left => user removed from index"
        );

        // Check TLE #3 and #4 were appended to revealed
        let revealed_final = RevealedCommitments::<Test>::get(netuid, who)
            .expect("Should exist with final reveals");
        assert_eq!(
            revealed_final.len(),
            4,
            "We should have all 4 TLE items revealed in total"
        );

        // The final two items in `revealed_final` must be #3, #4
        let (third_bytes, _) = &revealed_final[2];
        let (fourth_bytes, _) = &revealed_final[3];
        let third_txt = sp_std::str::from_utf8(third_bytes).expect("utf-8 expected");
        let fourth_txt = sp_std::str::from_utf8(fourth_bytes).expect("utf-8 expected");

        assert!(
            third_txt.contains("Timelock #3"),
            "Expected TLE #3 among final reveals"
        );
        assert!(
            fourth_txt.contains("Timelock #4"),
            "Expected TLE #4 among final reveals"
        );
    });
}

#[allow(clippy::indexing_slicing)]
#[test]
fn mixed_timelocked_and_raw_fields_works() {
    new_test_ext().execute_with(|| {
        // -------------------------------------------
        // 1) Setup initial block number and user
        // -------------------------------------------
        let cur_block = 3u64.into();
        System::<Test>::set_block_number(cur_block);

        let who = 77;
        let netuid = NetUid::from(501);

        // -------------------------------------------
        // 2) Create raw fields and timelocked fields
        // -------------------------------------------
        // We'll use 2 raw fields, and 2 timelocked fields referencing
        // 2 Drand rounds (1000 and 2000) that we know have valid signatures.

        // Round constants:
        let round_1000 = 1000;
        let round_2000 = 2000;

        // (a) Timelock #1 => round=1000
        let tle_1_plaintext = b"TLE #1 => round=1000";
        let ciphertext_1 = produce_ciphertext(tle_1_plaintext, round_1000);
        let tle_1 = Data::TimelockEncrypted {
            encrypted: ciphertext_1,
            reveal_round: round_1000,
        };

        // (b) Timelock #2 => round=2000
        let tle_2_plaintext = b"TLE #2 => round=2000";
        let ciphertext_2 = produce_ciphertext(tle_2_plaintext, round_2000);
        let tle_2 = Data::TimelockEncrypted {
            encrypted: ciphertext_2,
            reveal_round: round_2000,
        };

        // (c) Two Raw fields
        let raw_1 = Data::Raw(b"Raw field #1".to_vec().try_into().expect("<= 128 bytes"));
        let raw_2 = Data::Raw(b"Raw field #2".to_vec().try_into().expect("<= 128 bytes"));

        // We'll put them in a single vector: [TLE #1, raw_1, TLE #2, raw_2]
        let all_fields = vec![tle_1, raw_1.clone(), tle_2, raw_2.clone()];
        let fields_bounded = BoundedVec::try_from(all_fields).expect("<= MaxFields");

        // -------------------------------------------
        // 3) Submit the single commitment
        // -------------------------------------------
        let info = CommitmentInfo { fields: fields_bounded };

        assert_ok!(Pallet::<Test>::set_commitment(
            RuntimeOrigin::signed(who),
            netuid,
            Box::new(info)
        ));

        // The user should appear in TimelockedIndex because they have timelocked fields.
        assert!(
            TimelockedIndex::<Test>::get().contains(&(netuid, who)),
            "User must be in TimelockedIndex with TLE fields"
        );

        // Check the stored data
        let stored = CommitmentOf::<Test>::get(netuid, who).expect("Should exist in storage");
        assert_eq!(
            stored.info.fields.len(),
            4,
            "We have 2 raw + 2 TLE fields in total"
        );

        // -------------------------------------------
        // 4) Insert Drand signature for round=1000 => partial reveal
        // -------------------------------------------
        let drand_sig_1000 = hex::decode(DRAND_QUICKNET_SIG_HEX).expect("decode signature");
        insert_drand_pulse(round_1000, &drand_sig_1000);

        System::<Test>::set_block_number(4u64.into());
        assert_ok!(Pallet::<Test>::reveal_timelocked_commitments());

        // => TLE #1 (round=1000) is revealed. TLE #2 (round=2000) remains locked.
        // => The two raw fields remain untouched.
        let leftover_after_1000 = CommitmentOf::<Test>::get(netuid, who).expect("Must still exist");
        assert_eq!(
            leftover_after_1000.info.fields.len(),
            3,
            "One TLE removed => leftover=3 fields: TLE #2 + raw_1 + raw_2"
        );

        // Make sure user is still in TimelockedIndex (they still have TLE #2)
        assert!(
            TimelockedIndex::<Test>::get().contains(&(netuid, who)),
            "Still has leftover TLE #2 => remains in index"
        );

        // Check partial reveal
        let revealed_1000 = RevealedCommitments::<Test>::get(netuid, who)
            .expect("Should have partial reveals");
        assert_eq!(
            revealed_1000.len(),
            1,
            "We revealed exactly 1 item at round=1000"
        );
        let (revealed_bytes_1, _block_1) = &revealed_1000[0];
        let revealed_str_1 =
            sp_std::str::from_utf8(revealed_bytes_1).expect("Should parse as UTF-8");
        assert!(
            revealed_str_1.contains("TLE #1 => round=1000"),
            "Check that TLE #1 was revealed"
        );

        // -------------------------------------------
        // 5) Insert Drand signature for round=2000 => final TLE reveal
        // -------------------------------------------
        let drand_sig_2000_hex =
            "b6cb8f482a0b15d45936a4c4ea08e98a087e71787caee3f4d07a8a9843b1bc5423c6b3c22f446488b3137eaca799c77e";
        let drand_sig_2000 = hex::decode(drand_sig_2000_hex).expect("decode signature");
        insert_drand_pulse(round_2000, &drand_sig_2000);

        System::<Test>::set_block_number(5u64.into());
        assert_ok!(Pallet::<Test>::reveal_timelocked_commitments());

        // => TLE #2 is now revealed. The two raw fields remain.
        let leftover_after_2000 = CommitmentOf::<Test>::get(netuid, who).expect("Still exists");
        let leftover_fields = &leftover_after_2000.info.fields;
        assert_eq!(
            leftover_fields.len(),
            2,
            "Only the 2 raw fields remain after TLE #2 is revealed"
        );

        assert_eq!(
            leftover_fields[0],
            raw_1,
            "Leftover field[0] must match raw_1"
        );
        assert_eq!(
            leftover_fields[1],
            raw_2,
            "Leftover field[1] must match raw_2"
        );

        // The user has no leftover timelocks => removed from TimelockedIndex
        assert!(
            !TimelockedIndex::<Test>::get().contains(&(netuid, who)),
            "No more TLE => user removed from index"
        );

        // But the record is still present in storage (because raw fields remain)
        // => leftover_fields must match our original raw fields.
        let [f1, f2] = &leftover_fields[..] else {
            panic!("Expected exactly 2 fields leftover");
        };
        assert_eq!(f1, &raw_1, "Raw field #1 remains unaltered");
        assert_eq!(f2, &raw_2, "Raw field #2 remains unaltered");

        // Check that TLE #2 was appended to revealed data
        let revealed_final = RevealedCommitments::<Test>::get(netuid, who)
            .expect("Should have final reveals");
        assert_eq!(
            revealed_final.len(),
            2,
            "Now we have 2 revealed TLE items total (TLE #1 and TLE #2)."
        );
        let (revealed_bytes_2, _block_2) = &revealed_final[1];
        let revealed_str_2 =
            sp_std::str::from_utf8(revealed_bytes_2).expect("Should parse as UTF-8");
        assert!(
            revealed_str_2.contains("TLE #2 => round=2000"),
            "Check that TLE #2 was revealed"
        );
    });
}
