#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::unwrap_used)]
//! `RegisteredSubnetCounter` bumps across dissolve / re-registration.

use super::prelude::*;

#[test]
fn registered_subnet_counter_bumps_on_first_registration() {
    new_test_ext(1).execute_with(|| {
        let cold = U256::from(1);
        let hot = U256::from(2);

        let netuid = add_dynamic_network(&hot, &cold);

        assert_eq!(
            SubtensorModule::get_registered_subnet_counter(netuid),
            1,
            "first registration of a netuid must leave counter == 1"
        );
    });
}

#[test]
fn registered_subnet_counter_is_independent_per_netuid() {
    new_test_ext(1).execute_with(|| {
        let n1 = add_dynamic_network(&U256::from(10), &U256::from(11));
        let n2 = add_dynamic_network(&U256::from(20), &U256::from(21));

        assert_ne!(n1, n2);
        assert_eq!(SubtensorModule::get_registered_subnet_counter(n1), 1);
        assert_eq!(SubtensorModule::get_registered_subnet_counter(n2), 1);
    });
}

#[test]
fn registered_subnet_counter_survives_dissolve_and_bumps_on_reregistration() {
    new_test_ext(1).execute_with(|| {
        // Force reuse of the same netuid on re-registration by pinning the
        // active subnet cap so the next registration must prune.
        SubtensorModule::set_max_subnets(2);

        let owner_cold = U256::from(100);
        let owner_hot = U256::from(101);
        let netuid = add_dynamic_network(&owner_hot, &owner_cold);
        assert_eq!(SubtensorModule::get_registered_subnet_counter(netuid), 1);

        // Dissolve: counter is intentionally *not* cleared — stale consumers
        // can still detect the pre-dereg lifetime if they stored the counter
        // value they observed at approval time.
        assert_ok!(SubtensorModule::do_dissolve_network(netuid));
        run_block_idle();
        assert!(!SubtensorModule::subnet_exists(netuid));
        assert_eq!(
            SubtensorModule::get_registered_subnet_counter(netuid),
            1,
            "dissolve must not clear or reset the counter"
        );

        // Re-register. With the cap pinned, the prune selector reuses the
        // freed netuid; the counter bumps to 2 so that any state still keyed
        // to the prior value becomes unreachable under the new registration.
        let reg_netuid = add_dynamic_network(&owner_hot, &owner_cold);
        assert_eq!(
            reg_netuid, netuid,
            "the pruned netuid should be reused under the subnet cap"
        );
        assert_eq!(
            SubtensorModule::get_registered_subnet_counter(netuid),
            2,
            "re-registration must bump counter"
        );
    });
}
