#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::unwrap_used)]
//! Async dissolve cleanup queue, on_idle phases, and pending-subnet account id.

use super::prelude::*;

#[test]
fn dissolve_defers_cleanup_until_on_idle() {
    new_test_ext(0).execute_with(|| {
        let owner_cold = U256::from(11);
        let owner_hot = U256::from(12);
        let net = add_dynamic_network(&owner_hot, &owner_cold);

        // Set up EVM association data to verify it gets cleaned up too.
        let evm_key = sp_core::H160::from_low_u64_be(42);
        SubtensorModule::set_associated_evm_address(net, 0u16, evm_key, 1u64);
        assert!(AssociatedEvmAddress::<Test>::contains_key(net, 0u16));
        assert!(!AssociatedUidsByEvmAddress::<Test>::get(net, evm_key).is_empty());

        assert!(SubnetOwner::<Test>::contains_key(net));
        assert!(SubnetOwnerHotkey::<Test>::contains_key(net));
        assert!(NetworkRegisteredAt::<Test>::contains_key(net));
        assert!(!DissolveCleanupQueue::<Test>::get().contains(&net));

        assert_ok!(SubtensorModule::do_dissolve_network(net));

        // Network is no longer considered "existing" but data is not cleaned yet.
        assert!(!SubtensorModule::subnet_exists(net));
        assert!(DissolveCleanupQueue::<Test>::get().contains(&net));
        assert!(SubnetOwner::<Test>::contains_key(net));
        assert!(NetworkRegisteredAt::<Test>::contains_key(net));
        // EVM data still present before on_idle cleanup.
        assert!(AssociatedEvmAddress::<Test>::contains_key(net, 0u16));
        assert!(!AssociatedUidsByEvmAddress::<Test>::get(net, evm_key).is_empty());

        // Cleanup happens in on_idle.
        run_block_idle();
        assert!(!NetworkRegisteredAt::<Test>::contains_key(net));
        assert!(!SubnetOwner::<Test>::contains_key(net));
        assert!(!DissolveCleanupQueue::<Test>::get().contains(&net));
        // EVM data cleaned up as part of NetworkMapParameters phase.
        assert!(!AssociatedEvmAddress::<Test>::contains_key(net, 0u16));
        assert!(AssociatedUidsByEvmAddress::<Test>::get(net, evm_key).is_empty());
    });
}

#[test]
fn get_subnet_account_id_some_while_dissolved_cleanup_pending() {
    new_test_ext(1).execute_with(|| {
        let cold = U256::from(44_001);
        let hot = U256::from(44_002);
        let net = add_dynamic_network(&hot, &cold);
        assert_ok!(SubtensorModule::do_dissolve_network(net));
        assert!(!SubtensorModule::subnet_exists(net));
        assert!(DissolveCleanupQueue::<Test>::get().contains(&net));
        assert!(
            SubtensorModule::get_subnet_account_id(net).is_some(),
            "subnet TAO account must stay derivable during async dissolve cleanup"
        );
    });
}

#[test]
fn dissolve_async_cleanup_leaves_phase_unset_until_idle_finishes() {
    new_test_ext(0).execute_with(|| {
        let owner_cold = U256::from(910);
        let owner_hot = U256::from(911);
        let net = add_dynamic_network(&owner_hot, &owner_cold);

        assert_ok!(SubtensorModule::do_dissolve_network(net));
        assert!(
            DissolveCleanupQueue::<Test>::get().contains(&net),
            "dissolved netuid should be queued for on_idle cleanup"
        );
        assert!(
            CurrentDissolveCleanupStatus::<Test>::get().is_none(),
            "global cleanup phase is only driven from on_idle (not from do_dissolve_network)"
        );

        run_block_idle();

        assert!(
            !DissolveCleanupQueue::<Test>::get().contains(&net),
            "idle cleanup should drain the dissolved net from the queue"
        );
        assert!(
            CurrentDissolveCleanupStatus::<Test>::get().is_none(),
            "when the queue is empty, global cleanup phase storage must be cleared"
        );
    });
}

#[test]
fn dissolve_full_on_idle_emits_dissolved_network_data_cleaned_and_clears_phase() {
    // `frame_system::Pallet::events()` stays empty at block #0 in the test externalities;
    // use a non-zero block like other event-asserting tests (`recycle_alpha`, etc.).
    new_test_ext(1).execute_with(|| {
        let owner_cold = U256::from(930);
        let owner_hot = U256::from(931);
        let net = add_dynamic_network(&owner_hot, &owner_cold);

        assert_ok!(SubtensorModule::do_dissolve_network(net));
        System::reset_events();
        run_block_idle();

        assert!(
            System::events().iter().any(|e| {
                matches!(
                    &e.event,
                    RuntimeEvent::SubtensorModule(Event::NetworkDissolveCleanupCompleted { netuid: n })
                        if *n == net
                )
            }),
            "expected NetworkDissolveCleanupCompleted after async dissolve pipeline"
        );
        assert!(
            CurrentDissolveCleanupStatus::<Test>::get().is_none(),
            "global cleanup phase storage must be cleared when the queue is empty"
        );
    });
}

#[test]
fn dissolve_two_networks_fifo_cleanup_drains_queue() {
    new_test_ext(0).execute_with(|| {
        let n1 = add_dynamic_network(&U256::from(940), &U256::from(941));
        let n2 = add_dynamic_network(&U256::from(942), &U256::from(943));

        assert_ok!(SubtensorModule::do_dissolve_network(n1));
        assert_ok!(SubtensorModule::do_dissolve_network(n2));
        assert_eq!(DissolveCleanupQueue::<Test>::get(), vec![n1, n2]);

        let mut guard = 0u32;
        while !DissolveCleanupQueue::<Test>::get().is_empty() {
            guard = guard.saturating_add(1);
            assert!(
                guard < 256,
                "dissolve cleanup should drain in finite idle passes (guard={guard})"
            );
            run_block_idle();
        }

        assert!(!SubtensorModule::subnet_exists(n1));
        assert!(!SubtensorModule::subnet_exists(n2));
        assert!(
            CurrentDissolveCleanupStatus::<Test>::get().is_none(),
            "no stale phase after queue drain"
        );
    });
}
