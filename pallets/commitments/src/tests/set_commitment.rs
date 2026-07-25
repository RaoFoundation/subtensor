//! Tests for commitments pallet: set commitment.

use super::*;

#[test]
fn set_commitment_works() {
    new_test_ext().execute_with(|| {
        System::<Test>::set_block_number(1);
        let info = Box::new(CommitmentInfo {
            fields: BoundedVec::try_from(vec![]).expect("Expected not to panic"),
        });

        assert_ok!(Pallet::<Test>::set_commitment(
            RuntimeOrigin::signed(1),
            1.into(),
            info.clone()
        ));

        let commitment =
            Pallet::<Test>::commitment_of(NetUid::from(1), 1).expect("Expected not to panic");
        let initial_deposit = <Test as Config>::InitialDeposit::get();
        assert_eq!(commitment.deposit, initial_deposit);
        assert_eq!(commitment.block, 1);
        assert_eq!(Pallet::<Test>::last_commitment(NetUid::from(1), 1), Some(1));
    });
}

#[test]
#[should_panic(expected = "BoundedVec::try_from failed")]
fn set_commitment_too_many_fields_panics() {
    new_test_ext().execute_with(|| {
        let max_fields: u32 = <Test as Config>::MaxFields::get();
        let fields = vec![Data::None; (max_fields + 1) as usize];

        // This line will panic when 'BoundedVec::try_from(...)' sees too many items.
        let info = Box::new(CommitmentInfo {
            fields: BoundedVec::try_from(fields).expect("BoundedVec::try_from failed"),
        });

        // We never get here, because the constructor panics above.
        let _ = Pallet::<Test>::set_commitment(
            frame_system::RawOrigin::Signed(1).into(),
            1.into(),
            info,
        );
    });
}

#[test]
fn set_commitment_updates_deposit() {
    new_test_ext().execute_with(|| {
        System::<Test>::set_block_number(1);
        let info1 = Box::new(CommitmentInfo {
            fields: BoundedVec::try_from(vec![Default::default(); 2])
                .expect("Expected not to panic"),
        });
        let info2 = Box::new(CommitmentInfo {
            fields: BoundedVec::try_from(vec![Default::default(); 3])
                .expect("Expected not to panic"),
        });

        assert_ok!(Pallet::<Test>::set_commitment(
            RuntimeOrigin::signed(1),
            1.into(),
            info1
        ));
        let initial_deposit = <Test as Config>::InitialDeposit::get();
        let field_deposit = <Test as Config>::FieldDeposit::get();
        let expected_deposit1 = initial_deposit + field_deposit * 2.into();
        assert_eq!(
            Pallet::<Test>::commitment_of(NetUid::from(1), 1)
                .expect("Expected not to panic")
                .deposit,
            expected_deposit1
        );

        assert_ok!(Pallet::<Test>::set_commitment(
            RuntimeOrigin::signed(1),
            1.into(),
            info2
        ));
        let expected_deposit2 = initial_deposit + field_deposit * 3.into();
        assert_eq!(
            Pallet::<Test>::commitment_of(NetUid::from(1), 1)
                .expect("Expected not to panic")
                .deposit,
            expected_deposit2
        );
    });
}

#[test]
fn event_emission_works() {
    new_test_ext().execute_with(|| {
        System::<Test>::set_block_number(1);
        let info = Box::new(CommitmentInfo {
            fields: BoundedVec::try_from(vec![]).expect("Expected not to panic"),
        });

        assert_ok!(Pallet::<Test>::set_commitment(
            RuntimeOrigin::signed(1),
            1.into(),
            info
        ));

        let events = System::<Test>::events();
        let expected_event = RuntimeEvent::Commitments(Event::Commitment {
            netuid: 1.into(),
            who: 1,
        });
        assert!(events.iter().any(|e| e.event == expected_event));
    });
}

#[test]
fn set_commitment_unreserve_leftover_fails() {
    new_test_ext().execute_with(|| {
        use frame_system::RawOrigin;

        let netuid = NetUid::from(999);
        let who = 99;

        Balances::make_free_balance_be(&who, 10_000.into());

        let fake_deposit: TaoBalance = 100.into();
        let dummy_info = CommitmentInfo::<TestMaxFields> {
            fields: BoundedVec::try_from(vec![]).expect("empty fields is fine"),
        };
        let registration = Registration::<TaoBalance, TestMaxFields, u64> {
            deposit: fake_deposit,
            info: dummy_info,
            block: 0u64.into(),
        };

        CommitmentOf::<Test>::insert(netuid, who, registration);

        assert_ok!(Balances::reserve(&who, fake_deposit));
        assert_eq!(Balances::reserved_balance(who), 100.into());

        Balances::unreserve(&who, 10_000.into());
        assert_eq!(Balances::reserved_balance(who), 0.into());

        let commit_small = Box::new(CommitmentInfo {
            fields: BoundedVec::try_from(vec![]).expect("no fields is fine"),
        });

        assert_noop!(
            Pallet::<Test>::set_commitment(RawOrigin::Signed(who).into(), netuid, commit_small),
            Error::<Test>::UnexpectedUnreserveLeftover
        );
    });
}

#[test]
fn usage_respects_minimum_of_100_bytes() {
    new_test_ext().execute_with(|| {
        MaxSpace::<Test>::set(1000);

        let netuid = NetUid::from(1);
        let who = 99;

        System::<Test>::set_block_number(1);

        let small_data = Data::Raw(vec![0u8; 50].try_into().expect("<=128 bytes for Raw"));
        let info_small = Box::new(CommitmentInfo {
            fields: BoundedVec::try_from(vec![small_data]).expect("Must not exceed MaxFields"),
        });

        let usage_before = UsedSpaceOf::<Test>::get(netuid, who).unwrap_or_default();
        assert_eq!(usage_before.used_space, 0);

        assert_ok!(Pallet::<Test>::set_commitment(
            RuntimeOrigin::signed(who),
            netuid,
            info_small
        ));

        let usage_after_small =
            UsedSpaceOf::<Test>::get(netuid, who).expect("expected to not panic");
        assert_eq!(
            usage_after_small.used_space, 100,
            "Usage must jump to 100 even though we only used 50 bytes"
        );

        let big_data = Data::Raw(vec![0u8; 110].try_into().expect("<=128 bytes for Raw"));
        let info_big = Box::new(CommitmentInfo {
            fields: BoundedVec::try_from(vec![big_data]).expect("Must not exceed MaxFields"),
        });

        assert_ok!(Pallet::<Test>::set_commitment(
            RuntimeOrigin::signed(who),
            netuid,
            info_big
        ));

        let usage_after_big = UsedSpaceOf::<Test>::get(netuid, who).expect("expected to not panic");
        assert_eq!(
            usage_after_big.used_space, 210,
            "Usage should be 100 + 110 = 210 in this epoch"
        );

        UsedSpaceOf::<Test>::remove(netuid, who);
        let usage_after_wipe = UsedSpaceOf::<Test>::get(netuid, who);
        assert!(
            usage_after_wipe.is_none(),
            "Expected `UsedSpaceOf` entry to be removed"
        );

        let bigger_data = Data::Raw(vec![0u8; 120].try_into().expect("<=128 bytes for Raw"));
        let info_bigger = Box::new(CommitmentInfo {
            fields: BoundedVec::try_from(vec![bigger_data]).expect("Must not exceed MaxFields"),
        });

        assert_ok!(Pallet::<Test>::set_commitment(
            RuntimeOrigin::signed(who),
            netuid,
            info_bigger
        ));

        let usage_after_reset =
            UsedSpaceOf::<Test>::get(netuid, who).expect("expected to not panic");
        assert_eq!(
            usage_after_reset.used_space, 120,
            "After wiping old usage, the new usage should be exactly 120"
        );
    });
}

#[test]
fn set_commitment_works_with_multiple_raw_fields() {
    new_test_ext().execute_with(|| {
        let cur_block = 10u64.into();
        System::<Test>::set_block_number(cur_block);
        let initial_deposit: BalanceOf<Test> = <Test as Config>::InitialDeposit::get();
        let field_deposit: BalanceOf<Test> = <Test as Config>::FieldDeposit::get();

        let field1 = Data::Raw(vec![0u8; 10].try_into().expect("<=128 bytes is OK"));
        let field2 = Data::Raw(vec![1u8; 20].try_into().expect("<=128 bytes is OK"));
        let field3 = Data::Raw(vec![2u8; 50].try_into().expect("<=128 bytes is OK"));

        let info_multiple = CommitmentInfo {
            fields: BoundedVec::try_from(vec![field1.clone(), field2.clone(), field3.clone()])
                .expect("<= MaxFields"),
        };

        assert_ok!(Pallet::<Test>::set_commitment(
            RuntimeOrigin::signed(12345),
            99.into(),
            Box::new(info_multiple)
        ));

        let expected_deposit: BalanceOf<Test> = initial_deposit + field_deposit * 3u64.into();
        let stored = CommitmentOf::<Test>::get(NetUid::from(99), 12345).expect("Should be stored");
        assert_eq!(
            stored.deposit, expected_deposit,
            "Deposit must equal initial + 3 * field_deposit"
        );

        assert_eq!(stored.block, cur_block, "Stored block must match cur_block");

        let usage =
            UsedSpaceOf::<Test>::get(NetUid::from(99), 12345).expect("Expected to not panic");
        assert_eq!(
            usage.used_space, 100,
            "Usage is clamped to 100 when sum of fields is < 100"
        );

        let next_block = 11u64.into();
        System::<Test>::set_block_number(next_block);

        let info_two_fields = CommitmentInfo {
            fields: BoundedVec::try_from(vec![field1.clone(), field2.clone()])
                .expect("<= MaxFields"),
        };

        assert_ok!(Pallet::<Test>::set_commitment(
            RuntimeOrigin::signed(12345),
            99.into(),
            Box::new(info_two_fields)
        ));

        let expected_deposit2: BalanceOf<Test> = initial_deposit + field_deposit * 2u64.into();
        let stored2 = CommitmentOf::<Test>::get(NetUid::from(99), 12345).expect("Should be stored");
        assert_eq!(
            stored2.deposit, expected_deposit2,
            "Deposit must have decreased after removing one field"
        );

        let usage2 =
            UsedSpaceOf::<Test>::get(NetUid::from(99), 12345).expect("Expected to not panic");
        let expected_usage2 = 200u64;
        assert_eq!(
            usage2.used_space, expected_usage2,
            "Usage accumulates in the same epoch, respecting the min usage of 100 each time"
        );

        let events = System::<Test>::events();
        let expected_event = RuntimeEvent::Commitments(Event::Commitment {
            netuid: 99.into(),
            who: 12345,
        });
        let found_commitment_event = events.iter().any(|e| e.event == expected_event);
        assert!(
            found_commitment_event,
            "Expected at least one Event::Commitment to be emitted"
        );
    });
}
