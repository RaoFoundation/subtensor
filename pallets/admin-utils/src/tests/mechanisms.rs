//! Per-subnet mechanism count, emission splits, and global max mechanism count.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    unused_imports
)]

use super::prelude::*;

#[test]
fn test_sudo_set_mechanism_count() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        let ss_count_ok = MaxMechanismCount::<Test>::get();
        let ss_count_bad = MechId::from(u8::from(ss_count_ok) + 1);

        let sn_owner = U256::from(1324);
        add_network(netuid, 10);
        // Set the Subnet Owner
        SubnetOwner::<Test>::insert(netuid, sn_owner);
        MaxAllowedUids::<Test>::insert(netuid, 256_u16);

        assert_eq!(
            AdminUtils::sudo_set_mechanism_count(
                <<Test as Config>::RuntimeOrigin>::signed(U256::from(1)),
                netuid,
                ss_count_ok
            ),
            Err(DispatchError::BadOrigin)
        );
        assert_noop!(
            AdminUtils::sudo_set_mechanism_count(RuntimeOrigin::root(), netuid, ss_count_bad),
            pallet_subtensor::Error::<Test>::InvalidValue
        );
        assert_noop!(
            AdminUtils::sudo_set_mechanism_count(RuntimeOrigin::root(), netuid, ss_count_ok),
            pallet_subtensor::Error::<Test>::TooManyUIDsPerMechanism
        );

        // Reduce max UIDs to 128
        MaxAllowedUids::<Test>::insert(netuid, 128_u16);
        assert_ok!(AdminUtils::sudo_set_mechanism_count(
            <<Test as Config>::RuntimeOrigin>::root(),
            netuid,
            ss_count_ok
        ));

        assert_ok!(AdminUtils::sudo_set_mechanism_count(
            <<Test as Config>::RuntimeOrigin>::signed(sn_owner),
            netuid,
            ss_count_ok
        ));
    });
}

// cargo test --package pallet-admin-utils --lib -- tests::test_sudo_set_mechanism_count_and_emissions --exact --show-output
#[test]
fn test_sudo_set_mechanism_count_and_emissions() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        let ss_count_ok = MechId::from(2);

        let sn_owner = U256::from(1324);
        add_network(netuid, 10);
        // Set the Subnet Owner
        SubnetOwner::<Test>::insert(netuid, sn_owner);
        MaxMechanismCount::<Test>::set(MechId::from(2));
        MaxAllowedUids::<Test>::set(netuid, 128_u16);

        assert_ok!(AdminUtils::sudo_set_mechanism_count(
            <<Test as Config>::RuntimeOrigin>::signed(sn_owner),
            netuid,
            ss_count_ok
        ));

        // Cannot set emission split with wrong number of entries
        // With two mechanisms the size of the split vector should be 2, not 3
        assert_noop!(
            AdminUtils::sudo_set_mechanism_emission_split(
                <<Test as Config>::RuntimeOrigin>::signed(sn_owner),
                netuid,
                Some(vec![0xFFFF / 5 * 2, 0xFFFF / 5 * 2, 0xFFFF / 5])
            ),
            pallet_subtensor::Error::<Test>::InvalidValue
        );

        // Cannot set emission split with wrong total of entries
        // Split vector entries should sum up to exactly 0xFFFF
        assert_noop!(
            AdminUtils::sudo_set_mechanism_emission_split(
                <<Test as Config>::RuntimeOrigin>::signed(sn_owner),
                netuid,
                Some(vec![0xFFFF / 5 * 4, 0xFFFF / 5 - 1])
            ),
            pallet_subtensor::Error::<Test>::InvalidValue
        );

        // Can set good split ok
        // We also verify here that it can happen in the same block as setting mechanism counts
        // or soon, without rate limiting
        assert_ok!(AdminUtils::sudo_set_mechanism_emission_split(
            <<Test as Config>::RuntimeOrigin>::signed(sn_owner),
            netuid,
            Some(vec![0xFFFF / 5, 0xFFFF / 5 * 4])
        ));

        // Cannot set it again due to rate limits
        assert_noop!(
            AdminUtils::sudo_set_mechanism_emission_split(
                <<Test as Config>::RuntimeOrigin>::signed(sn_owner),
                netuid,
                Some(vec![0xFFFF / 5 * 4, 0xFFFF / 5])
            ),
            pallet_subtensor::Error::<Test>::TxRateLimitExceeded
        );
    });
}

#[test]
fn test_sudo_set_max_mechanism_count() {
    new_test_ext().execute_with(|| {
        // Normal case
        assert_ok!(AdminUtils::sudo_set_max_mechanism_count(
            <<Test as Config>::RuntimeOrigin>::root(),
            MechId::from(10)
        ));

        // Zero fails
        assert_noop!(
            AdminUtils::sudo_set_max_mechanism_count(
                <<Test as Config>::RuntimeOrigin>::root(),
                MechId::from(0)
            ),
            pallet_subtensor::Error::<Test>::InvalidValue
        );

        // Over max bound fails
        assert_noop!(
            AdminUtils::sudo_set_max_mechanism_count(
                <<Test as Config>::RuntimeOrigin>::root(),
                MechId::from(MAX_MECHANISM_COUNT_PER_SUBNET + 1)
            ),
            pallet_subtensor::Error::<Test>::InvalidValue
        );
    });
}
