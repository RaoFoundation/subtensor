use super::*;

fn set_pow_difficulty(
    origin: RuntimeOrigin,
    netuid: NetUid,
    value: u64,
    minimum: bool,
) -> sp_runtime::DispatchResult {
    if minimum {
        AdminUtils::sudo_set_min_difficulty(origin, netuid, value)
    } else {
        AdminUtils::sudo_set_difficulty(origin, netuid, value)
    }
}

fn pow_difficulty(netuid: NetUid, minimum: bool) -> u64 {
    if minimum {
        SubtensorModule::get_min_difficulty(netuid)
    } else {
        SubtensorModule::get_difficulty_as_u64(netuid)
    }
}

#[test]
fn pow_difficulty_owner_authority_is_scoped_to_its_subnet() {
    for minimum in [true, false] {
        new_test_ext().execute_with(|| {
            let netuid = NetUid::from(1);
            let other_netuid = NetUid::from(2);
            add_network(netuid, 10);
            add_network(other_netuid, 10);
            let owner = U256::from(9);
            SubnetOwner::<Test>::insert(netuid, owner);
            SubnetOwner::<Test>::insert(other_netuid, U256::from(10));
            let other_before = pow_difficulty(other_netuid, minimum);
            assert_noop!(
                set_pow_difficulty(RuntimeOrigin::signed(owner), other_netuid, 100, minimum),
                DispatchError::BadOrigin
            );
            assert_eq!(pow_difficulty(other_netuid, minimum), other_before);
            assert_noop!(
                set_pow_difficulty(RuntimeOrigin::none(), netuid, 100, minimum),
                DispatchError::BadOrigin
            );
            assert_ok!(set_pow_difficulty(
                RuntimeOrigin::signed(owner),
                netuid,
                100,
                minimum
            ));
            assert_eq!(pow_difficulty(netuid, minimum), 100);
        });
    }
}

#[test]
fn pow_difficulty_owner_cooldowns_are_independent_and_root_can_override() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 10);
        let owner = U256::from(9);
        SubnetOwner::<Test>::insert(netuid, owner);
        pallet_subtensor::OwnerHyperparamRateLimit::<Test>::put(5);
        System::set_block_number(1);
        // This is the pair needed to reopen a subnet previously pinned at MAX.
        assert_ok!(AdminUtils::sudo_set_min_difficulty(
            RuntimeOrigin::signed(owner),
            netuid,
            100
        ));
        assert_ok!(AdminUtils::sudo_set_difficulty(
            RuntimeOrigin::signed(owner),
            netuid,
            100
        ));
        for minimum in [true, false] {
            assert_noop!(
                set_pow_difficulty(RuntimeOrigin::signed(owner), netuid, 200, minimum),
                SubtensorError::<Test>::TxRateLimitExceeded
            );
            assert_eq!(pow_difficulty(netuid, minimum), 100);
            assert_ok!(set_pow_difficulty(
                RuntimeOrigin::root(),
                netuid,
                200,
                minimum
            ));
            assert_eq!(pow_difficulty(netuid, minimum), 200);
        }
        run_to_block(52);
        for minimum in [true, false] {
            assert_ok!(set_pow_difficulty(
                RuntimeOrigin::signed(owner),
                netuid,
                300,
                minimum
            ));
            assert_eq!(pow_difficulty(netuid, minimum), 300);
        }
    });
}

#[test]
fn pow_difficulty_freeze_rejects_owner_and_root_without_consuming_cooldown() {
    for minimum in [true, false] {
        new_test_ext().execute_with(|| {
            let netuid = NetUid::from(1);
            add_network(netuid, 10);
            let owner = U256::from(9);
            SubnetOwner::<Test>::insert(netuid, owner);
            pallet_subtensor::OwnerHyperparamRateLimit::<Test>::put(5);
            pallet_subtensor::LastEpochBlock::<Test>::insert(netuid, 0);
            assert_ok!(AdminUtils::sudo_set_admin_freeze_window(
                RuntimeOrigin::root(),
                3
            ));
            run_to_block(8);
            let before = pow_difficulty(netuid, minimum);
            for origin in [RuntimeOrigin::signed(owner), RuntimeOrigin::root()] {
                assert_noop!(
                    set_pow_difficulty(origin, netuid, 100, minimum),
                    SubtensorError::<Test>::AdminActionProhibitedDuringWeightsWindow
                );
            }
            assert_eq!(pow_difficulty(netuid, minimum), before);
            assert_ok!(AdminUtils::sudo_set_admin_freeze_window(
                RuntimeOrigin::root(),
                0
            ));
            assert_ok!(set_pow_difficulty(
                RuntimeOrigin::signed(owner),
                netuid,
                100,
                minimum
            ));
        });
    }
}

#[test]
fn pow_difficulty_root_subnet_remains_governance_only() {
    for minimum in [true, false] {
        new_test_ext().execute_with(|| {
            add_network(NetUid::ROOT, 10);
            let owner = U256::from(9);
            SubnetOwner::<Test>::insert(NetUid::ROOT, owner);
            assert_noop!(
                set_pow_difficulty(RuntimeOrigin::signed(owner), NetUid::ROOT, 100, minimum),
                Error::<Test>::NotPermittedOnRootSubnet
            );
            assert_ok!(set_pow_difficulty(
                RuntimeOrigin::root(),
                NetUid::ROOT,
                100,
                minimum
            ));
            assert_eq!(pow_difficulty(NetUid::ROOT, minimum), 100);
        });
    }
}

#[test]
fn pow_difficulty_weights_cover_the_measured_owner_storage_envelope() {
    let reference =
        <<Test as crate::Config>::WeightInfo as crate::WeightInfo>::sudo_set_adjustment_alpha();
    let calls = [
        crate::Call::<Test>::sudo_set_min_difficulty {
            netuid: NetUid::from(1),
            min_difficulty: 100,
        },
        crate::Call::<Test>::sudo_set_difficulty {
            netuid: NetUid::from(1),
            difficulty: 100,
        },
    ];
    for call in calls {
        let declared = call.get_dispatch_info().call_weight;
        assert!(declared.ref_time() >= reference.ref_time());
        assert!(declared.proof_size() >= reference.proof_size());
    }
}
