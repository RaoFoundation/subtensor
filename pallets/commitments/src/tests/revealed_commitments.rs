//! RevealedCommitments retention and on_initialize auto-reveal hook.

use super::*;

#[allow(clippy::indexing_slicing)]
#[test]
fn on_initialize_reveals_matured_timelocks() {
    new_test_ext().execute_with(|| {
        let who = 42;
        let netuid = NetUid::from(7);
        let reveal_round = 1000;

        let message_text = b"Timelock test via on_initialize";

        let inner_fields: BoundedVec<Data, <Test as Config>::MaxFields> =
            BoundedVec::try_from(vec![Data::Raw(
                message_text
                    .to_vec()
                    .try_into()
                    .expect("<= 128 bytes is OK for Data::Raw"),
            )])
            .expect("Should not exceed MaxFields");

        let inner_info: CommitmentInfo<<Test as Config>::MaxFields> = CommitmentInfo {
            fields: inner_fields,
        };

        let plaintext = inner_info.encode();
        let encrypted = produce_ciphertext(&plaintext, reveal_round);

        let outer_fields = BoundedVec::try_from(vec![Data::TimelockEncrypted {
            encrypted,
            reveal_round,
        }])
        .expect("One field is well under MaxFields");
        let info_outer = CommitmentInfo {
            fields: outer_fields,
        };

        System::<Test>::set_block_number(1);
        assert_ok!(Pallet::<Test>::set_commitment(
            RuntimeOrigin::signed(who),
            netuid,
            Box::new(info_outer)
        ));

        assert!(CommitmentOf::<Test>::get(netuid, who).is_some());
        assert!(
            TimelockedIndex::<Test>::get().contains(&(netuid, who)),
            "Should appear in TimelockedIndex since it contains a timelock"
        );

        let drand_sig_hex = hex::decode(DRAND_QUICKNET_SIG_HEX)
            .expect("Decoding DRAND_QUICKNET_SIG_HEX must not fail");
        insert_drand_pulse(reveal_round, &drand_sig_hex);

        assert!(RevealedCommitments::<Test>::get(netuid, who).is_none());

        System::<Test>::set_block_number(2);
        let weight = <Pallet<Test> as Hooks<u64>>::on_initialize(2);
        let expected_weight = <Test as Config>::WeightInfo::reveal_timelocked_commitments()
            .saturating_add(RocksDbWeight::get().reads(5))
            .saturating_add(RocksDbWeight::get().writes(3));
        assert_eq!(weight, expected_weight);

        let revealed_opt = RevealedCommitments::<Test>::get(netuid, who);
        assert!(
            revealed_opt.is_some(),
            "Expected that the timelock got revealed at block #2"
        );

        let leftover = CommitmentOf::<Test>::get(netuid, who);
        assert!(
            leftover.is_none(),
            "After revealing the only timelock, the entire commitment is removed."
        );

        assert!(
            !TimelockedIndex::<Test>::get().contains(&(netuid, who)),
            "No longer in TimelockedIndex after reveal."
        );

        let (revealed_bytes, reveal_block) =
            revealed_opt.expect("expected to not panic")[0].clone();
        assert_eq!(reveal_block, 2, "Should have revealed at block #2");

        let revealed_str = sp_std::str::from_utf8(&revealed_bytes)
            .expect("Expected valid UTF-8 in the revealed bytes for this test");

        let original_str =
            sp_std::str::from_utf8(message_text).expect("`message_text` is valid UTF-8");
        assert!(
            revealed_str.contains(original_str),
            "Revealed data must contain the original message text."
        );
    });
}

#[allow(clippy::indexing_slicing)]
#[test]
fn reveal_timelocked_bad_timelocks_are_removed() {
    new_test_ext().execute_with(|| {
        //
        // 1) Prepare multiple Data::TimelockEncrypted fields with different “badness” scenarios + one good field
        //
        // Round used for valid Drand signature
        let valid_round = 1000;
        // Round used for intentionally invalid Drand signature
        let invalid_sig_round = 999;
        // Round that has *no* Drand pulse => timelock remains stored, not revealed yet
        let no_pulse_round = 2001;

        // (a) TLE #1: Round=999 => Drand pulse *exists* but signature is invalid => skip/deleted
        let plaintext_1 = b"BadSignature";
        let ciphertext_1 = produce_ciphertext(plaintext_1, invalid_sig_round);
        let tle_bad_sig = Data::TimelockEncrypted {
            encrypted: ciphertext_1,
            reveal_round: invalid_sig_round,
        };

        // (b) TLE #2: Round=1000 => Drand signature is valid, but ciphertext is corrupted => skip/deleted
        let plaintext_2 = b"CorruptedCiphertext";
        let good_ct_2 = produce_ciphertext(plaintext_2, valid_round);
        let mut corrupted_ct_2 = good_ct_2.into_inner();
        if !corrupted_ct_2.is_empty() {
            corrupted_ct_2[0] ^= 0xFF; // flip a byte
        }
        let tle_corrupted = Data::TimelockEncrypted {
            encrypted: corrupted_ct_2.try_into().expect("Expected not to panic"),
            reveal_round: valid_round,
        };

        // (c) TLE #3: Round=1000 => Drand signature valid, ciphertext good, *but* plaintext is empty => skip/deleted
        let empty_good_ct = produce_ciphertext(&[], valid_round);
        let tle_empty_plaintext = Data::TimelockEncrypted {
            encrypted: empty_good_ct,
            reveal_round: valid_round,
        };

        // (d) TLE #4: Round=1000 => Drand signature valid, ciphertext valid, nonempty plaintext => should be revealed
        let plaintext_4 = b"Hello, I decrypt fine!";
        let good_ct_4 = produce_ciphertext(plaintext_4, valid_round);
        let tle_good = Data::TimelockEncrypted {
            encrypted: good_ct_4,
            reveal_round: valid_round,
        };

        // (e) TLE #5: Round=2001 => no Drand pulse => remains in storage
        let plaintext_5 = b"Still waiting for next round!";
        let good_ct_5 = produce_ciphertext(plaintext_5, no_pulse_round);
        let tle_no_pulse = Data::TimelockEncrypted {
            encrypted: good_ct_5,
            reveal_round: no_pulse_round,
        };

        //
        // 2) Assemble them all in one CommitmentInfo
        //
        let fields = vec![
            tle_bad_sig,         // #1
            tle_corrupted,       // #2
            tle_empty_plaintext, // #3
            tle_good,            // #4
            tle_no_pulse,        // #5
        ];
        let fields_bounded = BoundedVec::try_from(fields).expect("Should not exceed MaxFields");
        let info = CommitmentInfo {
            fields: fields_bounded,
        };

        //
        // 3) Insert the commitment
        //
        let who = 123;
        let netuid = NetUid::from(777);
        System::<Test>::set_block_number(1);
        assert_ok!(Pallet::<Test>::set_commitment(
            RawOrigin::Signed(who).into(),
            netuid,
            Box::new(info)
        ));

        //
        // 4) Insert pulses:
        //    - Round=999 => invalid signature => attempts to parse => fails => remove TLE #1
        //    - Round=1000 => valid signature => TLE #2 is corrupted => remove; #3 empty => remove; #4 reveals successfully
        //    - Round=2001 => no signature => TLE #5 remains
        //
        let bad_sig = [0x33u8; 10]; // obviously invalid for TinyBLS
        insert_drand_pulse(invalid_sig_round, &bad_sig);

        let drand_sig_1000 = hex::decode(DRAND_QUICKNET_SIG_HEX).expect("Expected not to panic");
        insert_drand_pulse(valid_round, &drand_sig_1000);

        //
        // 5) Call reveal => “bad” items are removed, “good” is revealed, “not ready” remains
        //
        System::<Test>::set_block_number(2);
        assert_ok!(Pallet::<Test>::reveal_timelocked_commitments());

        //
        // 6) Check final storage
        //
        // (a) TLE #5 => still in fields => same user remains in CommitmentOf => TimelockedIndex includes them
        let registration_after =
            CommitmentOf::<Test>::get(netuid, who).expect("Should still exist");
        assert_eq!(
            registration_after.info.fields.len(),
            1,
            "Only the unrevealed TLE #5 should remain"
        );
        let leftover = &registration_after.info.fields[0];
        match leftover {
            Data::TimelockEncrypted { reveal_round, .. } => {
                assert_eq!(*reveal_round, no_pulse_round, "Should be TLE #5 leftover");
            }
            _ => panic!("Expected the leftover field to be TLE #5"),
        };
        assert!(
            TimelockedIndex::<Test>::get().contains(&(netuid, who)),
            "Still in index because there's one remaining timelock (#5)."
        );

        // (b) TLE #4 => revealed => check that the plaintext matches
        let revealed = RevealedCommitments::<Test>::get(netuid, who)
            .expect("Should have at least one revealed item for TLE #4");
        let (revealed_bytes, reveal_block) = &revealed[0];
        assert_eq!(*reveal_block, 2, "Revealed at block #2");

        let revealed_str = sp_std::str::from_utf8(revealed_bytes)
            .expect("Truncated bytes should be valid UTF-8 in this test");

        let original_str =
            sp_std::str::from_utf8(plaintext_4).expect("plaintext_4 should be valid UTF-8");

        assert_eq!(
            revealed_str, original_str,
            "Expected revealed data to match the original plaintext"
        );

        // (c) TLE #1 / #2 / #3 => removed => do NOT appear in leftover fields, nor in revealed (they were invalid)
        assert_eq!(revealed.len(), 1, "Only TLE #4 ended up in revealed list");
    });
}

#[test]
fn revealed_commitments_keeps_only_10_items() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        let who = 2;
        let reveal_round = 1000;

        let drand_sig_bytes = hex::decode(DRAND_QUICKNET_SIG_HEX).expect("Should decode DRAND sig");
        insert_drand_pulse(reveal_round, &drand_sig_bytes);

        // --- 1) Build 12 TimelockEncrypted fields ---
        // Each one has a unique plaintext "TLE #i"
        const TOTAL_TLES: usize = 12;
        let mut fields = Vec::with_capacity(TOTAL_TLES);

        for i in 0..TOTAL_TLES {
            let plaintext = format!("TLE #{i}").into_bytes();
            let ciphertext = produce_ciphertext(&plaintext, reveal_round);
            let timelock = Data::TimelockEncrypted {
                encrypted: ciphertext,
                reveal_round,
            };
            fields.push(timelock);
        }
        let fields_bounded = BoundedVec::try_from(fields).expect("Should not exceed MaxFields");
        let info = CommitmentInfo {
            fields: fields_bounded,
        };

        // --- 2) Set the commitment => 12 timelocks in storage ---
        System::<Test>::set_block_number(1);
        assert_ok!(Pallet::<Test>::set_commitment(
            RuntimeOrigin::signed(who),
            netuid,
            Box::new(info)
        ));

        // --- 3) Reveal => all 12 are decrypted in one shot ---
        System::<Test>::set_block_number(2);
        assert_ok!(Pallet::<Test>::reveal_timelocked_commitments());

        // --- 4) Check we only keep 10 in `RevealedCommitments` ---
        let revealed = RevealedCommitments::<Test>::get(netuid, who)
            .expect("Should have at least some revealed data");
        assert_eq!(
            revealed.len(),
            10,
            "We must only keep the newest 10, out of 12 total"
        );

        // The oldest 2 ("TLE #0" and "TLE #1") must be dropped.
        // The items in `revealed` now correspond to "TLE #2" .. "TLE #11".
        for (idx, (revealed_bytes, reveal_block)) in revealed.iter().enumerate() {
            // Convert to UTF-8
            let revealed_str = sp_std::str::from_utf8(revealed_bytes)
                .expect("Decrypted data should be valid UTF-8 for this test case");

            // We expect them to be TLE #2..TLE #11
            let expected_index = idx + 2; // since we dropped #0 and #1
            let expected_str = format!("TLE #{expected_index}");
            assert_eq!(revealed_str, expected_str, "Check which TLE is kept");

            // Also check it was revealed at block 2
            assert_eq!(*reveal_block, 2, "All reveal in the same block #2");
        }
    });
}

#[test]
fn revealed_commitments_keeps_only_10_newest_with_individual_single_field_commits() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        let who = 2;
        let reveal_round = 1000;

        let drand_sig_bytes = hex::decode(DRAND_QUICKNET_SIG_HEX).expect("decode DRAND sig");
        insert_drand_pulse(reveal_round, &drand_sig_bytes);

        // We will add 12 separate timelocks, one per iteration, each in its own set_commitment call.
        // After each insertion, we call reveal + increment the block by 1.

        for i in 0..12 {
            System::<Test>::set_block_number(i as u64 + 1);

            let plaintext = format!("TLE #{i}").into_bytes();
            let ciphertext = produce_ciphertext(&plaintext, reveal_round);

            let new_timelock = Data::TimelockEncrypted {
                encrypted: ciphertext,
                reveal_round,
            };

            let fields = BoundedVec::try_from(vec![new_timelock])
                .expect("Single field is well within MaxFields");
            let info = CommitmentInfo { fields };

            assert_ok!(Pallet::<Test>::set_commitment(
                RuntimeOrigin::signed(who),
                netuid,
                Box::new(info)
            ));

            assert_ok!(Pallet::<Test>::reveal_timelocked_commitments());

            let revealed = RevealedCommitments::<Test>::get(netuid, who).unwrap_or_default();
            let expected_count = (i + 1).min(10);
            assert_eq!(
                revealed.len(),
                expected_count,
                "At iteration {i}, we keep at most 10 reveals"
            );
        }

        let revealed =
            RevealedCommitments::<Test>::get(netuid, who).expect("expected to not panic");
        assert_eq!(
            revealed.len(),
            10,
            "After 12 total commits, only 10 remain revealed"
        );

        // Check that TLE #0 and TLE #1 are dropped; TLE #2..#11 remain in ascending order.
        for (idx, (revealed_bytes, reveal_block)) in revealed.iter().enumerate() {
            let revealed_str =
                sp_std::str::from_utf8(revealed_bytes).expect("Should be valid UTF-8");
            let expected_i = idx + 2; // i=0 => "TLE #2", i=1 => "TLE #3", etc.
            let expected_str = format!("TLE #{expected_i}");

            assert_eq!(
                revealed_str, expected_str,
                "Revealed data #{idx} should match the truncated TLE #{expected_i}"
            );

            let expected_reveal_block = expected_i as u64 + 1;
            assert_eq!(
                *reveal_block, expected_reveal_block,
                "Check which block TLE #{expected_i} was revealed in"
            );
        }
    });
}
