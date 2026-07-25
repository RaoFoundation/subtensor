#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::unwrap_used)]
//! Subnet prune selection by price, immunity, and registration time.

use super::prelude::*;

#[test]
fn prune_none_with_no_networks() {
    new_test_ext(0).execute_with(|| {
        assert_eq!(SubtensorModule::get_network_to_prune(), None);
    });
}

#[test]
fn prune_none_when_all_networks_immune() {
    new_test_ext(0).execute_with(|| {
        // two fresh networks → still inside immunity window
        let n1 = add_dynamic_network(&U256::from(2), &U256::from(1));
        let _n2 = add_dynamic_network(&U256::from(4), &U256::from(3));

        // emissions don’t matter while immune
        Emission::<Test>::insert(n1, vec![AlphaBalance::from(10)]);

        assert_eq!(SubtensorModule::get_network_to_prune(), None);
    });
}

#[test]
fn prune_selects_network_with_lowest_price() {
    new_test_ext(0).execute_with(|| {
        let n1 = add_dynamic_network(&U256::from(20), &U256::from(10));
        let n2 = add_dynamic_network(&U256::from(40), &U256::from(30));

        // make both networks eligible (past immunity)
        let imm = SubtensorModule::get_network_immunity_period();
        System::set_block_number(imm + 10);

        // n1 has lower price → should be pruned
        SubnetMovingPrice::<Test>::insert(n1, I96F32::from_num(1));
        SubnetMovingPrice::<Test>::insert(n2, I96F32::from_num(10));

        assert_eq!(SubtensorModule::get_network_to_prune(), Some(n1));
    });
}

#[test]
fn prune_ignores_immune_network_even_if_lower_price() {
    new_test_ext(0).execute_with(|| {
        // create mature network n1 first
        let n1 = add_dynamic_network(&U256::from(22), &U256::from(11));

        let imm = SubtensorModule::get_network_immunity_period();
        System::set_block_number(imm + 5); // advance → n1 now mature

        // create second network n2 *inside* immunity
        let n2 = add_dynamic_network(&U256::from(44), &U256::from(33));

        // prices: n2 lower but immune; n1 must be selected
        SubnetMovingPrice::<Test>::insert(n1, I96F32::from_num(5));
        SubnetMovingPrice::<Test>::insert(n2, I96F32::from_num(1));

        System::set_block_number(imm + 10); // still immune for n2
        assert_eq!(SubtensorModule::get_network_to_prune(), Some(n1));
    });
}

#[test]
fn prune_tie_on_price_earlier_registration_wins() {
    new_test_ext(0).execute_with(|| {
        // n1 registered first
        let n1 = add_dynamic_network(&U256::from(66), &U256::from(55));

        // advance 1 block, then register n2 (later timestamp)
        System::set_block_number(1);
        let n2 = add_dynamic_network(&U256::from(88), &U256::from(77));

        // push past immunity for both
        let imm = SubtensorModule::get_network_immunity_period();
        System::set_block_number(imm + 20);

        // identical prices → tie; earlier (n1) must be chosen
        SubnetMovingPrice::<Test>::insert(n1, I96F32::from_num(7));
        SubnetMovingPrice::<Test>::insert(n2, I96F32::from_num(7));

        assert_eq!(SubtensorModule::get_network_to_prune(), Some(n1));
    });
}

#[test]
fn prune_selection_complex_state_exhaustive() {
    new_test_ext(0).execute_with(|| {
        let imm = SubtensorModule::get_network_immunity_period();

        // ---------------------------------------------------------------------
        // Build a rich topology of networks with controlled registration times.
        // ---------------------------------------------------------------------
        // n1 + n2 in the same block (equal timestamp) to test "tie + same time".
        System::set_block_number(0);
        let n1 = add_dynamic_network(&U256::from(101), &U256::from(201));
        let n2 = add_dynamic_network(&U256::from(102), &U256::from(202)); // same registered_at as n1

        // Later registrations (strictly greater timestamp than n1/n2)
        System::set_block_number(1);
        let n3 = add_dynamic_network(&U256::from(103), &U256::from(203));

        System::set_block_number(2);
        let n4 = add_dynamic_network(&U256::from(104), &U256::from(204));

        // Create *immune* networks that will remain ineligible initially,
        // even if their price is the lowest.
        System::set_block_number(imm + 5);
        let n5 = add_dynamic_network(&U256::from(105), &U256::from(205)); // immune at first

        System::set_block_number(imm + 6);
        let n6 = add_dynamic_network(&U256::from(106), &U256::from(206)); // immune at first

        // Add 100 TAO to subnet accounts (lock)
        let subnet_account1 = SubtensorModule::get_subnet_account_id(n1).unwrap();
        let subnet_account2 = SubtensorModule::get_subnet_account_id(n2).unwrap();
        let subnet_account3 = SubtensorModule::get_subnet_account_id(n3).unwrap();
        let subnet_account4 = SubtensorModule::get_subnet_account_id(n4).unwrap();
        let subnet_account5 = SubtensorModule::get_subnet_account_id(n5).unwrap();
        let subnet_account6 = SubtensorModule::get_subnet_account_id(n6).unwrap();
        add_balance_to_coldkey_account(&subnet_account1, 100_000_000_000_u64.into());
        add_balance_to_coldkey_account(&subnet_account2, 100_000_000_000_u64.into());
        add_balance_to_coldkey_account(&subnet_account3, 100_000_000_000_u64.into());
        add_balance_to_coldkey_account(&subnet_account4, 100_000_000_000_u64.into());
        add_balance_to_coldkey_account(&subnet_account5, 100_000_000_000_u64.into());
        add_balance_to_coldkey_account(&subnet_account6, 100_000_000_000_u64.into());

        // (Root is ignored by the selector.)
        let root = NetUid::ROOT;

        // ---------------------------------------------------------------------
        // Drive pruning via the EMA/moving price used by `get_network_to_prune()`.
        // We set the moving prices directly to create deterministic selections.
        //
        // Intended prices:
        // n1: 25, n2: 25, n3: 100, n4: 1, n5: 0 (immune initially), n6: 0 (immune initially)
        // ---------------------------------------------------------------------
        SubnetMovingPrice::<Test>::insert(n1, I96F32::from_num(25));
        SubnetMovingPrice::<Test>::insert(n2, I96F32::from_num(25));
        SubnetMovingPrice::<Test>::insert(n3, I96F32::from_num(100));
        SubnetMovingPrice::<Test>::insert(n4, I96F32::from_num(1));
        SubnetMovingPrice::<Test>::insert(n5, I96F32::from_num(0));
        SubnetMovingPrice::<Test>::insert(n6, I96F32::from_num(0));

        // ---------------------------------------------------------------------
        // Phase A: Only n1..n4 are mature → lowest price (n4=1) should win.
        // ---------------------------------------------------------------------
        System::set_block_number(imm + 10);
        assert_eq!(
            SubtensorModule::get_network_to_prune(),
            Some(n4),
            "Among mature nets (n1..n4), n4 has price=1 (lowest) and should be chosen."
        );

        // ---------------------------------------------------------------------
        // Phase B: Tie on price with *same registration time* (n1 vs n2).
        // Raise n4's price to 25 so {n1=25, n2=25, n3=100, n4=25}.
        // n1 and n2 share the *same registered_at*. The tie should keep the
        // first encountered (stable iteration by key order) → n1.
        // ---------------------------------------------------------------------
        SubnetMovingPrice::<Test>::insert(n4, I96F32::from_num(25)); // n4 now 25
        assert_eq!(
            SubtensorModule::get_network_to_prune(),
            Some(n1),
            "Tie on price with equal timestamps (n1,n2) → first encountered (n1) should persist."
        );

        // ---------------------------------------------------------------------
        // Phase C: Tie on price with *different registration times*.
        // Make n3 price=25 as well. Now n1,n2,n3,n4 all have price=25.
        // Earliest registration among them is n1 (block 0).
        // ---------------------------------------------------------------------
        SubnetMovingPrice::<Test>::insert(n3, I96F32::from_num(25));
        assert_eq!(
            SubtensorModule::get_network_to_prune(),
            Some(n1),
            "Tie on price across multiple nets → earliest registration (n1) wins."
        );

        // ---------------------------------------------------------------------
        // Phase D: Immune networks ignored even if strictly cheaper (0).
        // n5 and n6 price=0 but still immune at (imm + 10). Ensure they are
        // ignored and selection remains n1.
        // ---------------------------------------------------------------------
        let now = System::block_number();
        assert!(
            now < NetworkRegisteredAt::<Test>::get(n5) + imm,
            "n5 is immune at current block"
        );
        assert!(
            now < NetworkRegisteredAt::<Test>::get(n6) + imm,
            "n6 is immune at current block"
        );
        assert_eq!(
            SubtensorModule::get_network_to_prune(),
            Some(n1),
            "Immune nets (n5,n6) must be ignored despite lower price."
        );

        // ---------------------------------------------------------------------
        // Phase E: If *all* networks are immune → return None.
        // Move clock back before any network's immunity expires.
        // ---------------------------------------------------------------------
        System::set_block_number(0);
        assert_eq!(
            SubtensorModule::get_network_to_prune(),
            None,
            "With all networks immune, there is no prunable candidate."
        );

        // ---------------------------------------------------------------------
        // Phase F: Advance beyond immunity for n5 & n6.
        // Both n5 and n6 now eligible with price=0 (lowest).
        // Tie on price; earlier registration between n5 and n6 is n5.
        // ---------------------------------------------------------------------
        System::set_block_number(2 * imm + 10);
        assert!(
            System::block_number() >= NetworkRegisteredAt::<Test>::get(n5) + imm,
            "n5 has matured"
        );
        assert!(
            System::block_number() >= NetworkRegisteredAt::<Test>::get(n6) + imm,
            "n6 has matured"
        );
        assert_eq!(
            SubtensorModule::get_network_to_prune(),
            Some(n5),
            "After immunity, n5 (price=0) should win; tie with n6 broken by earlier registration."
        );

        // ---------------------------------------------------------------------
        // Phase G: Create *sparse* netuids and ensure selection is stable.
        // Remove n5; now n6 (price=0) should be selected.
        // This validates robustness to holes / non-contiguous netuids.
        // ---------------------------------------------------------------------
        assert_ok!(SubtensorModule::do_dissolve_network(n5));
        assert_eq!(
            SubtensorModule::get_network_to_prune(),
            Some(n6),
            "After removing n5, next-lowest (n6=0) should be chosen even with sparse netuids."
        );

        // ---------------------------------------------------------------------
        // Phase H: Dynamic price changes.
        // Make n6 expensive (price 100); make n3 cheapest (price 1).
        // ---------------------------------------------------------------------
        SubnetMovingPrice::<Test>::insert(n6, I96F32::from_num(100));
        SubnetMovingPrice::<Test>::insert(n3, I96F32::from_num(1));
        assert_eq!(
            SubtensorModule::get_network_to_prune(),
            Some(n3),
            "Dynamic changes: n3 set to price=1 (lowest among eligibles) → should be pruned."
        );

        // ---------------------------------------------------------------------
        // Phase I: Tie again (n2 vs n3) but earlier registration must win.
        // Give n2 the same price as n3; n2 registered at block 0, n3 at block 1.
        // n2 should be chosen.
        // ---------------------------------------------------------------------
        SubnetMovingPrice::<Test>::insert(n2, I96F32::from_num(1));
        assert_eq!(
            SubtensorModule::get_network_to_prune(),
            Some(n2),
            "Tie on price across n2 (earlier reg) and n3 → n2 wins by timestamp."
        );

        // ---------------------------------------------------------------------
        // (Extra) Mark n2 as 'not added' to assert we honor the `added` flag,
        // then restore it to avoid side-effects on subsequent tests.
        // ---------------------------------------------------------------------
        NetworksAdded::<Test>::insert(n2, false);
        assert_ne!(
            SubtensorModule::get_network_to_prune(),
            Some(n2),
            "`added=false` must exclude n2 from consideration."
        );
        NetworksAdded::<Test>::insert(n2, true);

        // Root is always ignored even if cheapest (get_moving_alpha_price returns 1 for ROOT).
        assert_ne!(
            SubtensorModule::get_network_to_prune(),
            Some(root),
            "ROOT must never be selected for pruning."
        );
    });
}
