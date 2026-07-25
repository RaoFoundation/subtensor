#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::unwrap_used)]
//! Subnet owner may validate/set weights without stake or manual permit.

use frame_support::assert_ok;
use sp_core::U256;

use crate::tests::mock::*;
use crate::*;

#[test]
fn test_subnet_owner_can_validate_without_stake_or_manual_permit() {
    new_test_ext(0).execute_with(|| {
        let owner_hotkey = U256::from(10);
        let owner_coldkey = U256::from(11);
        let other_hotkey = U256::from(20);
        let other_coldkey = U256::from(21);

        // Create a real dynamic subnet whose owner hotkey is `owner_hotkey`.
        let netuid = add_dynamic_network_disable_commit_reveal(&owner_hotkey, &owner_coldkey);
        remove_owner_registration_stake(netuid);

        // Add one non-owner neuron with deterministic subnet stake.
        register_ok_neuron(netuid, other_hotkey, other_coldkey, 0);
        add_balance_to_coldkey_account(&other_coldkey, 1.into());
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &other_hotkey,
            &other_coldkey,
            netuid,
            1.into(),
        );

        let owner_uid =
            SubtensorModule::get_owner_uid(netuid).expect("subnet owner should resolve to a uid");
        let registered_owner_uid =
            SubtensorModule::get_uid_for_net_and_hotkey(netuid, &owner_hotkey)
                .expect("owner hotkey should be registered on the subnet");
        assert_eq!(registered_owner_uid, owner_uid);

        let other_uid = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &other_hotkey)
            .expect("other hotkey should be registered on the subnet");

        let (owner_weight_stake, _, _) =
            SubtensorModule::get_stake_weights_for_hotkey_on_subnet(&owner_hotkey, netuid);
        let (other_weight_stake, _, _) =
            SubtensorModule::get_stake_weights_for_hotkey_on_subnet(&other_hotkey, netuid);
        assert!(owner_weight_stake < other_weight_stake);

        // Make the non-owner stake-qualified while the owner remains below threshold.
        SubtensorModule::set_stake_threshold(1_u64);
        assert!(SubtensorModule::check_weights_min_stake(
            &other_hotkey,
            netuid
        ));

        // Clear all explicit permits. The owner should not rely on manual permit state.
        SubtensorModule::set_validator_permit_for_uid(netuid, owner_uid, false);
        SubtensorModule::set_validator_permit_for_uid(netuid, other_uid, false);
        assert!(!SubtensorModule::get_validator_permit_for_uid(
            netuid, owner_uid
        ));
        assert!(!SubtensorModule::get_validator_permit_for_uid(
            netuid, other_uid
        ));

        // Sanity check: a non-owner without a permit still cannot set non-self weights.
        assert!(!SubtensorModule::check_validator_permit(
            netuid,
            other_uid,
            &[owner_uid],
            &[1u16],
        ));
        assert_eq!(
            SubtensorModule::set_weights(
                RuntimeOrigin::signed(other_hotkey),
                netuid,
                vec![owner_uid],
                vec![1u16],
                0,
            ),
            Err(Error::<Test>::NeuronNoValidatorPermit.into())
        );

        // The subnet owner bypasses both the stake gate and the validator-permit gate.
        assert!(SubtensorModule::check_weights_min_stake(
            &owner_hotkey,
            netuid
        ));
        assert!(SubtensorModule::check_validator_permit(
            netuid,
            owner_uid,
            &[other_uid],
            &[1u16],
        ));

        assert_ok!(SubtensorModule::set_weights(
            RuntimeOrigin::signed(owner_hotkey),
            netuid,
            vec![other_uid],
            vec![1u16],
            0,
        ));

        // After an epoch, the owner is still validator-eligible even though only the
        step_epochs(1, netuid);
        assert!(SubtensorModule::get_validator_permit_for_uid(
            netuid, owner_uid
        ));

        // The original top-k result is preserved; the owner is added on top.
        assert!(SubtensorModule::get_validator_permit_for_uid(
            netuid, other_uid
        ));
    });
}

// Regression: when a batch of weight commits has per-item failures, each
// emitted BatchWeightItemFailed event must carry the netuid of the failing
// item so downstream consumers (indexers, validator monitors) can correlate
// failure → subnet without re-deriving from iteration order.
//
// Both netuids in this test fail (commit-reveal disabled on both) — what we
// assert is the *positional propagation*: the per-item events carry the
// distinct netuids that produced them, in iteration order.
