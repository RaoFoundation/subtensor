#![allow(clippy::unwrap_used)]

use super::mock::*;
use crate::Error;
use frame_support::{assert_noop, assert_ok};
use pallet_commitments::CommitBurnConfig;
use pallet_subtensor::SubnetOwner;
use sp_core::U256;
use sp_runtime::DispatchError;
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::{MechId, NetUid, TaoBalance, Token};

#[test]
fn commit_burn_setters_default_free_and_validate_owner_mechanism_and_bounds() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        let owner = U256::from(99);
        add_network(netuid, 360);
        SubnetOwner::<Test>::insert(netuid, owner);
        SubtensorModule::set_admin_freeze_window(0);
        pallet_subtensor::OwnerHyperparamRateLimit::<Test>::put(0);
        let index = SubtensorModule::get_mechanism_storage_index(netuid, MechId::MAIN);
        assert!(CommitBurnConfig::<Test>::get((index, 0)).is_none());

        assert_noop!(
            AdminUtils::sudo_set_commit_burn_enabled(
                RuntimeOrigin::signed(U256::from(1)),
                netuid,
                MechId::MAIN,
                0,
                true,
            ),
            DispatchError::BadOrigin
        );
        assert_noop!(
            AdminUtils::sudo_set_commit_burn_enabled(
                RuntimeOrigin::signed(owner),
                netuid,
                MechId::from(1),
                0,
                true,
            ),
            pallet_subtensor::Error::<Test>::MechanismDoesNotExist
        );
        assert_ok!(AdminUtils::sudo_set_commit_burn_enabled(
            RuntimeOrigin::signed(owner),
            netuid,
            MechId::MAIN,
            0,
            true,
        ));
        assert_eq!(
            SubtensorModule::get_commit_burn(netuid, MechId::MAIN, 0)
                .unwrap()
                .burn,
            TaoBalance::ZERO
        );
        assert_ok!(AdminUtils::sudo_set_commit_burn_max(
            RuntimeOrigin::signed(owner),
            netuid,
            MechId::MAIN,
            0,
            TaoBalance::from(10_000u64),
        ));
        assert_ok!(AdminUtils::sudo_set_commit_burn_min(
            RuntimeOrigin::signed(owner),
            netuid,
            MechId::MAIN,
            0,
            TaoBalance::from(1_000u64),
        ));
        assert_ok!(AdminUtils::sudo_set_commit_burn_half_life(
            RuntimeOrigin::signed(owner),
            netuid,
            MechId::MAIN,
            0,
            100,
        ));
        assert_ok!(AdminUtils::sudo_set_commit_burn_increase_mult(
            RuntimeOrigin::signed(owner),
            netuid,
            MechId::MAIN,
            0,
            U64F64::from_num(2),
        ));
        let config = CommitBurnConfig::<Test>::get((index, 0)).unwrap();
        assert_eq!(config.burn, 1_000u64.into());
        assert_eq!(config.half_life, 100);
        assert_eq!(config.increase_mult, U64F64::from_num(2));
        assert_noop!(
            AdminUtils::sudo_set_commit_burn_half_life(
                RuntimeOrigin::signed(owner),
                netuid,
                MechId::MAIN,
                0,
                0,
            ),
            Error::<Test>::InvalidValue
        );
        assert_noop!(
            AdminUtils::sudo_set_commit_burn_increase_mult(
                RuntimeOrigin::signed(owner),
                netuid,
                MechId::MAIN,
                0,
                U64F64::from_num(4),
            ),
            Error::<Test>::InvalidValue
        );
        assert_noop!(
            AdminUtils::sudo_set_commit_burn_max(
                RuntimeOrigin::signed(owner),
                netuid,
                MechId::MAIN,
                0,
                TaoBalance::from(1u64),
            ),
            Error::<Test>::InvalidValue
        );
    });
}
