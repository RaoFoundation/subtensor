//! Timelock reveal path: decrypt when drand pulse is available, leave immature entries untouched.

use super::*;

#[allow(clippy::indexing_slicing)]
#[test]
fn happy_path_timelock_commitments() {
    new_test_ext().execute_with(|| {
        let message_text = b"Hello timelock only!";
        let data_raw = Data::Raw(
            message_text
                .to_vec()
                .try_into()
                .expect("<= 128 bytes for Raw variant"),
        );
        let fields_vec = vec![data_raw];
        let fields_bounded: BoundedVec<Data, <Test as Config>::MaxFields> =
            BoundedVec::try_from(fields_vec).expect("Too many fields");

        let inner_info: CommitmentInfo<<Test as Config>::MaxFields> = CommitmentInfo {
            fields: fields_bounded,
        };

        let plaintext = inner_info.encode();

        let reveal_round = 1000;
        let encrypted = produce_ciphertext(&plaintext, reveal_round);

        let data = Data::TimelockEncrypted {
            encrypted: encrypted.clone(),
            reveal_round,
        };

        let fields_outer: BoundedVec<Data, <Test as Config>::MaxFields> =
            BoundedVec::try_from(vec![data]).expect("Too many fields");
        let info_outer = CommitmentInfo {
            fields: fields_outer,
        };

        let who = 123;
        let netuid = NetUid::from(42);
        System::<Test>::set_block_number(1);

        assert_ok!(Pallet::<Test>::set_commitment(
            RuntimeOrigin::signed(who),
            netuid,
            Box::new(info_outer)
        ));

        let drand_signature_bytes =
            hex::decode(DRAND_QUICKNET_SIG_HEX).expect("Expected not to panic");
        insert_drand_pulse(reveal_round, &drand_signature_bytes);

        System::<Test>::set_block_number(9999);
        assert_ok!(Pallet::<Test>::reveal_timelocked_commitments());

        let revealed =
            RevealedCommitments::<Test>::get(netuid, who).expect("Should have revealed data");

        let (revealed_bytes, _reveal_block) = revealed[0].clone();

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

#[test]
fn reveal_timelocked_commitment_missing_round_does_nothing() {
    new_test_ext().execute_with(|| {
        let who = 1;
        let netuid = NetUid::from(2);
        System::<Test>::set_block_number(5);
        let ciphertext = produce_ciphertext(b"My plaintext", 1000);
        let data = Data::TimelockEncrypted {
            encrypted: ciphertext,
            reveal_round: 1000,
        };
        let fields: BoundedVec<_, <Test as Config>::MaxFields> =
            BoundedVec::try_from(vec![data]).expect("Expected not to panic");
        let info = CommitmentInfo { fields };
        let origin = RuntimeOrigin::signed(who);
        assert_ok!(Pallet::<Test>::set_commitment(
            origin,
            netuid,
            Box::new(info)
        ));
        System::<Test>::set_block_number(100_000);
        assert_ok!(Pallet::<Test>::reveal_timelocked_commitments());
        assert!(RevealedCommitments::<Test>::get(netuid, who).is_none());
    });
}

#[allow(clippy::indexing_slicing)]
#[test]
fn reveal_timelocked_commitment_cant_deserialize_ciphertext() {
    new_test_ext().execute_with(|| {
        let who = 42;
        let netuid = NetUid::from(9);
        System::<Test>::set_block_number(10);
        let good_ct = produce_ciphertext(b"Some data", 1000);
        let mut corrupted = good_ct.into_inner();
        if !corrupted.is_empty() {
            corrupted[0] = 0xFF;
        }
        let corrupted_ct = BoundedVec::try_from(corrupted).expect("Expected not to panic");
        let data = Data::TimelockEncrypted {
            encrypted: corrupted_ct,
            reveal_round: 1000,
        };
        let fields = BoundedVec::try_from(vec![data]).expect("Expected not to panic");
        let info = CommitmentInfo { fields };
        let origin = RuntimeOrigin::signed(who);
        assert_ok!(Pallet::<Test>::set_commitment(
            origin,
            netuid,
            Box::new(info)
        ));
        let sig_bytes = hex::decode(DRAND_QUICKNET_SIG_HEX).expect("Expected not to panic");
        insert_drand_pulse(1000, &sig_bytes);
        System::<Test>::set_block_number(99999);
        assert_ok!(Pallet::<Test>::reveal_timelocked_commitments());
        assert!(RevealedCommitments::<Test>::get(netuid, who).is_none());
    });
}

#[test]
fn reveal_timelocked_commitment_bad_signature_skips_decryption() {
    new_test_ext().execute_with(|| {
        let who = 10;
        let netuid = NetUid::from(11);
        System::<Test>::set_block_number(15);
        let real_ct = produce_ciphertext(b"A valid plaintext", 1000);
        let data = Data::TimelockEncrypted {
            encrypted: real_ct,
            reveal_round: 1000,
        };
        let fields: BoundedVec<_, <Test as Config>::MaxFields> =
            BoundedVec::try_from(vec![data]).expect("Expected not to panic");
        let info = CommitmentInfo { fields };
        let origin = RuntimeOrigin::signed(who);
        assert_ok!(Pallet::<Test>::set_commitment(
            origin,
            netuid,
            Box::new(info)
        ));
        let bad_signature = [0x33u8; 10];
        insert_drand_pulse(1000, &bad_signature);
        System::<Test>::set_block_number(10_000);
        assert_ok!(Pallet::<Test>::reveal_timelocked_commitments());
        assert!(RevealedCommitments::<Test>::get(netuid, who).is_none());
    });
}

#[test]
fn reveal_timelocked_commitment_empty_decrypted_data_is_skipped() {
    new_test_ext().execute_with(|| {
        let who = 2;
        let netuid = NetUid::from(3);
        let commit_block = 100u64;
        System::<Test>::set_block_number(commit_block);
        let reveal_round = 1000;
        let empty_ct = produce_ciphertext(&[], reveal_round);
        let data = Data::TimelockEncrypted {
            encrypted: empty_ct,
            reveal_round,
        };
        let fields = BoundedVec::try_from(vec![data]).expect("Expected not to panic");
        let info = CommitmentInfo { fields };
        let origin = RuntimeOrigin::signed(who);
        assert_ok!(Pallet::<Test>::set_commitment(
            origin,
            netuid,
            Box::new(info)
        ));
        let sig_bytes = hex::decode(DRAND_QUICKNET_SIG_HEX).expect("Expected not to panic");
        insert_drand_pulse(reveal_round, &sig_bytes);
        System::<Test>::set_block_number(10_000);
        assert_ok!(Pallet::<Test>::reveal_timelocked_commitments());
        assert!(RevealedCommitments::<Test>::get(netuid, who).is_none());
    });
}

#[allow(clippy::indexing_slicing)]
#[test]
fn reveal_timelocked_commitment_single_field_entry_is_removed_after_reveal() {
    new_test_ext().execute_with(|| {
        let message_text = b"Single field timelock test!";
        let data_raw = Data::Raw(
            message_text
                .to_vec()
                .try_into()
                .expect("Message must be <=128 bytes for Raw variant"),
        );

        let fields_bounded: BoundedVec<Data, <Test as Config>::MaxFields> =
            BoundedVec::try_from(vec![data_raw]).expect("BoundedVec creation must not fail");

        let inner_info: CommitmentInfo<<Test as Config>::MaxFields> = CommitmentInfo {
            fields: fields_bounded,
        };

        let plaintext = inner_info.encode();
        let reveal_round = 1000;
        let encrypted = produce_ciphertext(&plaintext, reveal_round);

        let timelock_data = Data::TimelockEncrypted {
            encrypted,
            reveal_round,
        };
        let fields_outer: BoundedVec<Data, <Test as Config>::MaxFields> =
            BoundedVec::try_from(vec![timelock_data]).expect("Too many fields");
        let info_outer: CommitmentInfo<<Test as Config>::MaxFields> = CommitmentInfo {
            fields: fields_outer,
        };

        let who = 555;
        let netuid = NetUid::from(777);
        System::<Test>::set_block_number(1);
        assert_ok!(Pallet::<Test>::set_commitment(
            RuntimeOrigin::signed(who),
            netuid,
            Box::new(info_outer)
        ));

        let drand_signature_bytes = hex::decode(DRAND_QUICKNET_SIG_HEX)
            .expect("Must decode DRAND_QUICKNET_SIG_HEX successfully");
        insert_drand_pulse(reveal_round, &drand_signature_bytes);

        System::<Test>::set_block_number(9999);
        assert_ok!(Pallet::<Test>::reveal_timelocked_commitments());

        let revealed =
            RevealedCommitments::<Test>::get(netuid, who).expect("Expected to find revealed data");
        let (revealed_bytes, _reveal_block) = revealed[0].clone();

        // The decrypted bytes have some extra SCALE metadata in front:
        // we slice off the first two bytes before checking the string.
        let offset = 2;
        let truncated = &revealed_bytes[offset..];
        let revealed_str = sp_std::str::from_utf8(truncated)
            .expect("Truncated bytes should be valid UTF-8 in this test");

        let original_str =
            sp_std::str::from_utf8(message_text).expect("`message_text` should be valid UTF-8");
        assert_eq!(
            revealed_str, original_str,
            "Expected the revealed data (minus prefix) to match the original message"
        );
        assert!(
            crate::CommitmentOf::<Test>::get(netuid, who).is_none(),
            "Expected CommitmentOf<T> entry to be removed after reveal"
        );
    });
}

#[allow(clippy::indexing_slicing)]
#[test]
fn reveal_timelocked_multiple_fields_only_correct_ones_removed() {
    new_test_ext().execute_with(|| {
        let round_1000 = 1000;

        // 2) Build two CommitmentInfos, one for each timelock
        let msg_1 = b"Hello from TLE #1";
        let inner_1_fields: BoundedVec<Data, <Test as Config>::MaxFields> =
            BoundedVec::try_from(vec![Data::Raw(
                msg_1.to_vec().try_into().expect("expected not to panic"),
            )])
            .expect("BoundedVec of size 1");
        let inner_info_1 = CommitmentInfo {
            fields: inner_1_fields,
        };
        let encoded_1 = inner_info_1.encode();
        let ciphertext_1 = produce_ciphertext(&encoded_1, round_1000);
        let timelock_1 = Data::TimelockEncrypted {
            encrypted: ciphertext_1,
            reveal_round: round_1000,
        };

        let msg_2 = b"Hello from TLE #2";
        let inner_2_fields: BoundedVec<Data, <Test as Config>::MaxFields> =
            BoundedVec::try_from(vec![Data::Raw(
                msg_2.to_vec().try_into().expect("expected not to panic"),
            )])
            .expect("BoundedVec of size 1");
        let inner_info_2 = CommitmentInfo {
            fields: inner_2_fields,
        };
        let encoded_2 = inner_info_2.encode();
        let ciphertext_2 = produce_ciphertext(&encoded_2, round_1000);
        let timelock_2 = Data::TimelockEncrypted {
            encrypted: ciphertext_2,
            reveal_round: round_1000,
        };

        // 3) One plain Data::Raw field (non-timelocked)
        let raw_bytes = b"Plain non-timelocked data";
        let data_raw = Data::Raw(
            raw_bytes
                .to_vec()
                .try_into()
                .expect("expected not to panic"),
        );

        // 4) Outer commitment: 3 fields total => [Raw, TLE #1, TLE #2]
        let outer_fields = BoundedVec::try_from(vec![
            data_raw.clone(),
            timelock_1.clone(),
            timelock_2.clone(),
        ])
        .expect("T::MaxFields >= 3 in the test config, or at least 3 here");
        let outer_info = CommitmentInfo {
            fields: outer_fields,
        };

        // 5) Insert the commitment
        let who = 123;
        let netuid = NetUid::from(999);
        System::<Test>::set_block_number(1);
        assert_ok!(Pallet::<Test>::set_commitment(
            RuntimeOrigin::signed(who),
            netuid,
            Box::new(outer_info)
        ));
        let initial = Pallet::<Test>::commitment_of(netuid, who).expect("Must exist");
        assert_eq!(initial.info.fields.len(), 3, "3 fields inserted");

        // 6) Insert Drand signature for round=1000
        let drand_sig_1000 = hex::decode(DRAND_QUICKNET_SIG_HEX).expect("decode DRAND sig");
        insert_drand_pulse(round_1000, &drand_sig_1000);

        // 7) Reveal once
        System::<Test>::set_block_number(50);
        assert_ok!(Pallet::<Test>::reveal_timelocked_commitments());

        // => The pallet code has removed *both* TLE #1 and TLE #2 in this single call!
        let after_reveal = Pallet::<Test>::commitment_of(netuid, who)
            .expect("Should still exist with leftover fields");
        // Only the raw, non-timelocked field remains
        assert_eq!(
            after_reveal.info.fields.len(),
            1,
            "Both timelocks referencing round=1000 got removed at once"
        );
        assert_eq!(
            after_reveal.info.fields[0], data_raw,
            "Only the raw field is left"
        );

        // 8) Check revealed data
        let revealed_data = RevealedCommitments::<Test>::get(netuid, who)
            .expect("Expected revealed data for TLE #1 and #2");

        let (revealed_bytes1, reveal_block1) = revealed_data[0].clone();
        let (revealed_bytes2, reveal_block2) = revealed_data[1].clone();

        let truncated1 = &revealed_bytes1[2..];
        let truncated2 = &revealed_bytes2[2..];

        assert_eq!(truncated1, msg_1);
        assert_eq!(reveal_block1, 50);
        assert_eq!(truncated2, msg_2);
        assert_eq!(reveal_block2, 50);

        // 9) A second reveal call now does nothing, because no timelocks remain
        System::<Test>::set_block_number(51);
        assert_ok!(Pallet::<Test>::reveal_timelocked_commitments());

        let after_second = Pallet::<Test>::commitment_of(netuid, who).expect("Still must exist");
        assert_eq!(
            after_second.info.fields.len(),
            1,
            "No new fields were removed, because no timelocks remain"
        );
    });
}

#[test]
fn two_timelocks_partial_then_full_reveal() {
    new_test_ext().execute_with(|| {
        let netuid_a = NetUid::from(1);
        let who_a = 10;
        let round_1000 = 1000;
        let round_2000 = 2000;

        let drand_sig_1000 = hex::decode(DRAND_QUICKNET_SIG_HEX).expect("Expected success");
        insert_drand_pulse(round_1000, &drand_sig_1000);

        let drand_sig_2000_hex =
            "b6cb8f482a0b15d45936a4c4ea08e98a087e71787caee3f4d07a8a9843b1bc5423c6b3c22f446488b3137eaca799c77e";

        //
        // First Timelock => round=1000
        //
        let msg_a1 = b"UserA timelock #1 (round=1000)";
        let inner_1_fields: BoundedVec<Data, <Test as Config>::MaxFields> = BoundedVec::try_from(
            vec![Data::Raw(msg_a1.to_vec().try_into().expect("Expected success"))],
        )
        .expect("MaxFields >= 1");
        let inner_info_1: CommitmentInfo<<Test as Config>::MaxFields> = CommitmentInfo {
            fields: inner_1_fields,
        };
        let encoded_1 = inner_info_1.encode();
        let ciphertext_1 = produce_ciphertext(&encoded_1, round_1000);
        let tle_a1 = Data::TimelockEncrypted {
            encrypted: ciphertext_1,
            reveal_round: round_1000,
        };

        //
        // Second Timelock => round=2000
        //
        let msg_a2 = b"UserA timelock #2 (round=2000)";
        let inner_2_fields: BoundedVec<Data, <Test as Config>::MaxFields> = BoundedVec::try_from(
            vec![Data::Raw(msg_a2.to_vec().try_into().expect("Expected success"))],
        )
        .expect("MaxFields >= 1");
        let inner_info_2: CommitmentInfo<<Test as Config>::MaxFields> = CommitmentInfo {
            fields: inner_2_fields,
        };
        let encoded_2 = inner_info_2.encode();
        let ciphertext_2 = produce_ciphertext(&encoded_2, round_2000);
        let tle_a2 = Data::TimelockEncrypted {
            encrypted: ciphertext_2,
            reveal_round: round_2000,
        };

        //
        // Insert outer commitment with both timelocks
        //
        let fields_a: BoundedVec<Data, <Test as Config>::MaxFields> =
            BoundedVec::try_from(vec![tle_a1, tle_a2]).expect("2 fields, must be <= MaxFields");
        let info_a: CommitmentInfo<<Test as Config>::MaxFields> = CommitmentInfo { fields: fields_a };

        assert_ok!(Pallet::<Test>::set_commitment(
            RuntimeOrigin::signed(who_a),
            netuid_a,
            Box::new(info_a)
        ));
        assert!(
            TimelockedIndex::<Test>::get().contains(&(netuid_a, who_a)),
            "User A must be in index with 2 timelocks"
        );

        System::<Test>::set_block_number(10);
        assert_ok!(Pallet::<Test>::reveal_timelocked_commitments());

        let leftover_a1 = CommitmentOf::<Test>::get(netuid_a, who_a).expect("still there");
        assert_eq!(
            leftover_a1.info.fields.len(),
            1,
            "Only the round=1000 timelock removed; round=2000 remains"
        );
        assert!(
            TimelockedIndex::<Test>::get().contains(&(netuid_a, who_a)),
            "Still in index with leftover timelock"
        );

        //
        // Insert signature for round=2000 => final reveal => leftover=none => removed
        //
        let drand_sig_2000 = hex::decode(drand_sig_2000_hex).expect("Expected success");
        insert_drand_pulse(round_2000, &drand_sig_2000);

        System::<Test>::set_block_number(11);
        assert_ok!(Pallet::<Test>::reveal_timelocked_commitments());

        let leftover_a2 = CommitmentOf::<Test>::get(netuid_a, who_a);
        assert!(
            leftover_a2.is_none(),
            "All timelocks removed => none leftover"
        );
        assert!(
            !TimelockedIndex::<Test>::get().contains(&(netuid_a, who_a)),
            "User A removed from index after final reveal"
        );
    });
}

#[test]
fn single_timelock_reveal_later_round() {
    new_test_ext().execute_with(|| {
        let netuid_b = NetUid::from(2);
        let who_b = 20;
        let round_2000 = 2000;

        let drand_sig_2000_hex =
            "b6cb8f482a0b15d45936a4c4ea08e98a087e71787caee3f4d07a8a9843b1bc5423c6b3c22f446488b3137eaca799c77e";
        let drand_sig_2000 = hex::decode(drand_sig_2000_hex).expect("Expected success");
        insert_drand_pulse(round_2000, &drand_sig_2000);

        let msg_b = b"UserB single timelock (round=2000)";

        let inner_b_fields: BoundedVec<Data, <Test as Config>::MaxFields> =
            BoundedVec::try_from(vec![Data::Raw(msg_b.to_vec().try_into().expect("Expected success"))])
                .expect("MaxFields >= 1");
        let inner_info_b: CommitmentInfo<<Test as Config>::MaxFields> = CommitmentInfo {
            fields: inner_b_fields,
        };
        let encoded_b = inner_info_b.encode();
        let ciphertext_b = produce_ciphertext(&encoded_b, round_2000);
        let tle_b = Data::TimelockEncrypted {
            encrypted: ciphertext_b,
            reveal_round: round_2000,
        };

        let fields_b: BoundedVec<Data, <Test as Config>::MaxFields> =
            BoundedVec::try_from(vec![tle_b]).expect("1 field");
        let info_b: CommitmentInfo<<Test as Config>::MaxFields> = CommitmentInfo { fields: fields_b };

        assert_ok!(Pallet::<Test>::set_commitment(
            RuntimeOrigin::signed(who_b),
            netuid_b,
            Box::new(info_b)
        ));
        assert!(
            TimelockedIndex::<Test>::get().contains(&(netuid_b, who_b)),
            "User B in index"
        );

        // Remove the round=2000 signature so first reveal does nothing
        pallet_drand::Pulses::<Test>::remove(round_2000);

        System::<Test>::set_block_number(20);
        assert_ok!(Pallet::<Test>::reveal_timelocked_commitments());

        let leftover_b1 = CommitmentOf::<Test>::get(netuid_b, who_b).expect("still there");
        assert_eq!(
            leftover_b1.info.fields.len(),
            1,
            "No signature => timelock remains"
        );
        assert!(
            TimelockedIndex::<Test>::get().contains(&(netuid_b, who_b)),
            "Still in index with leftover timelock"
        );

        insert_drand_pulse(round_2000, &drand_sig_2000);

        System::<Test>::set_block_number(21);
        assert_ok!(Pallet::<Test>::reveal_timelocked_commitments());

        let leftover_b2 = CommitmentOf::<Test>::get(netuid_b, who_b);
        assert!(leftover_b2.is_none(), "Timelock removed => leftover=none");
        assert!(
            !TimelockedIndex::<Test>::get().contains(&(netuid_b, who_b)),
            "User B removed from index after final reveal"
        );
    });
}
