#![allow(clippy::expect_used)]
use frame_support::{assert_noop, assert_ok};
use frame_system::Config;
use sp_core::U256;
use subtensor_runtime_common::NetUid;

use super::mock::*;
use crate::{
    ActivityCutoffFactorMilli, AdminFreezeWindow, CommitRevealWeightsEnabled, LastEpochBlock,
    PendingEpochAt, SubnetOwner, SubtokenEnabled, Tempo,
};

const DEFAULT_TEMPO: u16 = 360;
const NEW_TEMPO: u16 = 720;

fn setup_subnet(owner: U256) -> NetUid {
    let netuid = NetUid::from(1);
    add_network(netuid, DEFAULT_TEMPO, 0);
    SubnetOwner::<Test>::insert(netuid, owner);
    SubtokenEnabled::<Test>::insert(netuid, true);
    crate::Pallet::<Test>::set_admin_freeze_window(0);
    netuid
}

#[test]
fn do_set_tempo_works_with_commit_reveal_enabled() {
    new_test_ext(1).execute_with(|| {
        let owner = U256::from(1);
        let netuid = setup_subnet(owner);

        // CR is enabled by default; `set_tempo` is no longer blocked for CR
        // subnets — CR timing keys off the stateful `SubnetEpochIndex` counter.
        assert!(CommitRevealWeightsEnabled::<Test>::get(netuid));

        assert_ok!(crate::Pallet::<Test>::do_set_tempo(
            <<Test as Config>::RuntimeOrigin>::signed(owner),
            netuid,
            NEW_TEMPO,
        ));

        assert_eq!(Tempo::<Test>::get(netuid), NEW_TEMPO);
    });
}

#[test]
fn do_trigger_epoch_blocked_with_commit_reveal_enabled() {
    new_test_ext(1).execute_with(|| {
        let owner = U256::from(1);
        let netuid = setup_subnet(owner);

        // CR enabled by default; an out-of-band epoch would desync the CRv3 reveal
        // window from the Drand schedule and drop committed weights, so it is blocked.
        assert!(CommitRevealWeightsEnabled::<Test>::get(netuid));
        AdminFreezeWindow::<Test>::set(5);

        assert_noop!(
            crate::Pallet::<Test>::do_trigger_epoch(
                <<Test as Config>::RuntimeOrigin>::signed(owner),
                netuid,
            ),
            crate::Error::<Test>::DynamicTempoBlockedByCommitReveal
        );

        // No pending epoch was scheduled.
        assert_eq!(PendingEpochAt::<Test>::get(netuid), 0);
    });
}

#[test]
fn do_trigger_epoch_works_with_commit_reveal_disabled() {
    new_test_ext(1).execute_with(|| {
        let owner = U256::from(1);
        let netuid = setup_subnet(owner);

        // With CR disabled there is no reveal window to protect, so the trigger fires.
        CommitRevealWeightsEnabled::<Test>::insert(netuid, false);
        AdminFreezeWindow::<Test>::set(5);

        assert_ok!(crate::Pallet::<Test>::do_trigger_epoch(
            <<Test as Config>::RuntimeOrigin>::signed(owner),
            netuid,
        ));

        let now = crate::Pallet::<Test>::get_current_block_as_u64();
        assert_eq!(PendingEpochAt::<Test>::get(netuid), now + 5);
    });
}

#[test]
fn do_set_activity_cutoff_factor_works_for_root_bypassing_freeze_window() {
    new_test_ext(1).execute_with(|| {
        let owner = U256::from(1);
        let netuid = setup_subnet(owner);

        // Engage the admin freeze window so an owner-call would fail.
        Tempo::<Test>::insert(netuid, 10u16);
        LastEpochBlock::<Test>::insert(netuid, 1u64);
        AdminFreezeWindow::<Test>::set(8);
        run_to_block(5);

        // Owner cannot bypass the freeze window.
        assert_noop!(
            crate::Pallet::<Test>::do_set_activity_cutoff_factor(
                <<Test as Config>::RuntimeOrigin>::signed(owner),
                netuid,
                5_000u32,
            ),
            crate::Error::<Test>::AdminActionProhibitedDuringWeightsWindow
        );

        // Root bypasses both freeze window and rate limit.
        assert_ok!(crate::Pallet::<Test>::do_set_activity_cutoff_factor(
            <<Test as Config>::RuntimeOrigin>::root(),
            netuid,
            5_000u32,
        ));
        assert_eq!(ActivityCutoffFactorMilli::<Test>::get(netuid), 5_000u32);
    });
}

#[test]
fn do_trigger_epoch_rejects_when_auto_epoch_already_imminent() {
    new_test_ext(1).execute_with(|| {
        let owner = U256::from(1);
        let netuid = setup_subnet(owner);

        // Disable CR so the trigger reaches the imminent-auto-epoch check rather than
        // being short-circuited by the commit-reveal guard.
        CommitRevealWeightsEnabled::<Test>::insert(netuid, false);

        // Make the next auto epoch closer than AdminFreezeWindow.
        // remaining = (LastEpochBlock + tempo) - now = (1 + 10) - 5 = 6, window = 8 => reject.
        Tempo::<Test>::insert(netuid, 10u16);
        LastEpochBlock::<Test>::insert(netuid, 1u64);
        AdminFreezeWindow::<Test>::set(8);
        run_to_block(5);

        assert_noop!(
            crate::Pallet::<Test>::do_trigger_epoch(
                <<Test as Config>::RuntimeOrigin>::signed(owner),
                netuid,
            ),
            crate::Error::<Test>::AutoEpochAlreadyImminent
        );

        // Nothing was scheduled.
        assert_eq!(PendingEpochAt::<Test>::get(netuid), 0);
    });
}

#[test]
fn get_next_epoch_start_block_returns_none_when_tempo_zero() {
    new_test_ext(1).execute_with(|| {
        let owner = U256::from(1);
        let netuid = setup_subnet(owner);

        Tempo::<Test>::insert(netuid, 0);

        assert_eq!(
            crate::Pallet::<Test>::get_next_epoch_start_block(netuid),
            None
        );
    });
}

#[test]
fn get_next_epoch_start_block_uses_last_epoch_block_plus_tempo() {
    new_test_ext(1).execute_with(|| {
        let owner = U256::from(1);
        let netuid = setup_subnet(owner);

        LastEpochBlock::<Test>::insert(netuid, 100u64);
        Tempo::<Test>::insert(netuid, 50u16);
        PendingEpochAt::<Test>::insert(netuid, 0u64);

        // last (100) + tempo (50) = 150
        assert_eq!(
            crate::Pallet::<Test>::get_next_epoch_start_block(netuid),
            Some(150)
        );
    });
}

#[test]
fn get_next_epoch_start_block_returns_pending_when_pending_is_earlier() {
    new_test_ext(1).execute_with(|| {
        let owner = U256::from(1);
        let netuid = setup_subnet(owner);

        LastEpochBlock::<Test>::insert(netuid, 100u64);
        Tempo::<Test>::insert(netuid, 50u16);
        // Owner-triggered manual fire scheduled before automatic next.
        PendingEpochAt::<Test>::insert(netuid, 120u64);

        // min(150, 120) = 120
        assert_eq!(
            crate::Pallet::<Test>::get_next_epoch_start_block(netuid),
            Some(120)
        );
    });
}

#[test]
fn get_next_epoch_start_block_ignores_pending_when_auto_is_earlier() {
    new_test_ext(1).execute_with(|| {
        let owner = U256::from(1);
        let netuid = setup_subnet(owner);

        LastEpochBlock::<Test>::insert(netuid, 100u64);
        Tempo::<Test>::insert(netuid, 50u16);
        // Pending scheduled after the next automatic fire.
        PendingEpochAt::<Test>::insert(netuid, 200u64);

        // min(150, 200) = 150
        assert_eq!(
            crate::Pallet::<Test>::get_next_epoch_start_block(netuid),
            Some(150)
        );
    });
}

#[test]
fn get_next_epoch_start_block_reflects_set_tempo_cycle_reset() {
    new_test_ext(1).execute_with(|| {
        let owner = U256::from(1);
        let netuid = setup_subnet(owner);

        run_to_block(10);
        let new_tempo: u16 = 720;

        assert_ok!(crate::Pallet::<Test>::do_set_tempo(
            <<Test as Config>::RuntimeOrigin>::signed(owner),
            netuid,
            new_tempo,
        ));

        let now = crate::Pallet::<Test>::get_current_block_as_u64();
        // apply_tempo_with_cycle_reset sets LastEpochBlock = now;
        // next fire is now + tempo.
        assert_eq!(
            crate::Pallet::<Test>::get_next_epoch_start_block(netuid),
            Some(now + new_tempo as u64)
        );
    });
}
