#![allow(
    unused,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used
)]
//! Root children dividend drain through coinbase.

use super::helpers::*;
use super::prelude::*;

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::root_children_drain::test_get_root_children --exact --show-output --nocapture
#[test]
fn test_get_root_children() {
    new_test_ext(1).execute_with(|| {
        // Init netuid 1
        let alpha = NetUid::from(1);
        add_network(NetUid::ROOT, 1, 0);
        add_network(alpha, 1, 0);

        // Set TAO weight to 1.
        SubtensorModule::set_tao_weight(u64::MAX); // Set TAO weight to 1.

        // Create keys.
        let cold = U256::from(0);
        let alice = U256::from(1);
        let bob = U256::from(2);

        // Register Alice and Bob to the root network and alpha subnet.
        register_ok_neuron(alpha, alice, cold, 0);
        register_ok_neuron(alpha, bob, cold, 0);
        assert_ok!(SubtensorModule::root_register(
            RuntimeOrigin::signed(cold).clone(),
            alice,
        ));
        assert_ok!(SubtensorModule::root_register(
            RuntimeOrigin::signed(cold).clone(),
            bob,
        ));

        // Add stake for Alice and Bob on root.
        let alice_root_stake = AlphaBalance::from(1_000_000_000);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &alice,
            &cold,
            NetUid::ROOT,
            alice_root_stake,
        );
        let bob_root_stake = AlphaBalance::from(1_000_000_000);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &bob,
            &cold,
            NetUid::ROOT,
            alice_root_stake,
        );

        // Add stake for Alice and Bob on netuid.
        let alice_alpha_stake = AlphaBalance::from(1_000_000_000);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &alice,
            &cold,
            alpha,
            alice_alpha_stake,
        );
        let bob_alpha_stake = AlphaBalance::from(1_000_000_000);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &bob,
            &cold,
            alpha,
            bob_alpha_stake,
        );

        // Set Bob as 100% child of Alice on root.
        // mock_set_children_no_epochs( NetUid::ROOT, &alice, &[(u64::MAX, bob)]);
        mock_set_children_no_epochs(alpha, &alice, &[(u64::MAX, bob)]);

        // Assert Alice and Bob stake on root and netuid
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_on_subnet(&alice, NetUid::ROOT),
            alice_root_stake
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_on_subnet(&bob, NetUid::ROOT),
            bob_root_stake
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_on_subnet(&alice, alpha),
            alice_alpha_stake
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_on_subnet(&bob, alpha),
            bob_alpha_stake
        );

        // Assert Alice and Bob inherited stakes
        assert_eq!(
            SubtensorModule::get_inherited_for_hotkey_on_subnet(&alice, NetUid::ROOT),
            alice_root_stake
        );
        assert_eq!(
            SubtensorModule::get_inherited_for_hotkey_on_subnet(&alice, alpha),
            0.into()
        );
        assert_eq!(
            SubtensorModule::get_inherited_for_hotkey_on_subnet(&bob, NetUid::ROOT),
            bob_root_stake
        );
        assert_eq!(
            SubtensorModule::get_inherited_for_hotkey_on_subnet(&bob, alpha),
            bob_alpha_stake + alice_alpha_stake
        );

        // Assert Alice and Bob TAO inherited stakes
        assert_eq!(
            SubtensorModule::get_tao_inherited_for_hotkey_on_subnet(&alice, alpha),
            TaoBalance::ZERO
        );
        assert_eq!(
            SubtensorModule::get_tao_inherited_for_hotkey_on_subnet(&bob, alpha),
            u64::from(bob_root_stake + alice_root_stake).into()
        );

        // Get Alice stake amounts on subnet alpha.
        let (alice_total, alice_alpha, alice_tao): (I64F64, I64F64, I64F64) =
            SubtensorModule::get_stake_weights_for_hotkey_on_subnet(&alice, alpha);
        assert_eq!(alice_total, I64F64::from_num(0));

        // Get Bob stake amounts on subnet alpha.
        let (bob_total, bob_alpha, bob_tao): (I64F64, I64F64, I64F64) =
            SubtensorModule::get_stake_weights_for_hotkey_on_subnet(&bob, alpha);
        assert_eq!(
            bob_total,
            I64F64::from_num(u64::from(bob_root_stake * 4.into()))
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::root_children_drain::test_get_root_children_drain --exact --show-output --nocapture
#[test]
fn test_get_root_children_drain() {
    new_test_ext(1).execute_with(|| {
        // Init netuid 1
        let alpha = NetUid::from(1);
        add_network(NetUid::ROOT, 1, 0);
        add_network(alpha, 1, 0);
        SubtensorModule::set_ck_burn(0);
        // Set TAO weight to 1.
        SubtensorModule::set_tao_weight(u64::MAX); // Set TAO weight to 1.
        // Create keys.
        let cold_alice = U256::from(0);
        let cold_bob = U256::from(1);
        let alice = U256::from(2);
        let bob = U256::from(3);
        // Register Alice and Bob to the root network and alpha subnet.
        register_ok_neuron(alpha, alice, cold_alice, 0);
        register_ok_neuron(alpha, bob, cold_bob, 0);
        assert_ok!(SubtensorModule::root_register(
            RuntimeOrigin::signed(cold_alice).clone(),
            alice,
        ));
        assert_ok!(SubtensorModule::root_register(
            RuntimeOrigin::signed(cold_bob).clone(),
            bob,
        ));
        // Add stake for Alice and Bob on root.
        let alice_root_stake = 1_000_000_000;
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &alice,
            &cold_alice,
            NetUid::ROOT,
            alice_root_stake.into(),
        );
        let bob_root_stake = 1_000_000_000;
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &bob,
            &cold_bob,
            NetUid::ROOT,
            bob_root_stake.into(),
        );
        // Add stake for Alice and Bob on netuid.
        let alice_alpha_stake = AlphaBalance::from(1_000_000_000);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &alice,
            &cold_alice,
            alpha,
            alice_alpha_stake,
        );
        let bob_alpha_stake = AlphaBalance::from(1_000_000_000);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &bob,
            &cold_bob,
            alpha,
            bob_alpha_stake,
        );
        // Set Bob as 100% child of Alice on root.
        mock_set_children_no_epochs(alpha, &alice, &[(u64::MAX, bob)]);
        // Set Bob childkey take to zero.
        ChildkeyTake::<Test>::insert(bob, alpha, PerU16::zero());
        Delegates::<Test>::insert(alice, PerU16::zero());
        Delegates::<Test>::insert(bob, PerU16::zero());

        // Get Alice stake amounts on subnet alpha.
        let (alice_total, alice_alpha, alice_tao): (I64F64, I64F64, I64F64) =
            SubtensorModule::get_stake_weights_for_hotkey_on_subnet(&alice, alpha);
        assert_eq!(alice_total, I64F64::from_num(0));

        // Get Bob stake amounts on subnet alpha.
        let (bob_total, bob_alpha, bob_tao): (I64F64, I64F64, I64F64) =
            SubtensorModule::get_stake_weights_for_hotkey_on_subnet(&bob, alpha);
        assert_eq!(bob_total, I64F64::from_num(4_u64 * bob_root_stake));

        // Lets drain
        let pending_alpha = AlphaBalance::from(1_000_000_000);
        SubtensorModule::distribute_emission(
            alpha,
            pending_alpha.saturating_div(2.into()).into(),
            pending_alpha.saturating_div(2.into()).into(),
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
        );

        // Alice and Bob both made half of the dividends.
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_on_subnet(&alice, alpha),
            alice_alpha_stake + pending_alpha / 2.into()
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_on_subnet(&bob, alpha),
            bob_alpha_stake + pending_alpha / 2.into()
        );

        // There should be no TAO on the root subnet.
        assert_eq!(SubnetTAO::<Test>::get(NetUid::ROOT), TaoBalance::ZERO);

        // Lets drain
        let pending_alpha = AlphaBalance::from(1_000_000_000);
        let pending_root1 = TaoBalance::from(1_000_000_000);
        SubtensorModule::distribute_emission(
            alpha,
            pending_alpha.saturating_div(2.into()).into(),
            pending_alpha.saturating_div(2.into()).into(),
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
        );

        // Alice and Bob both made half of the dividends.
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_on_subnet(&alice, NetUid::ROOT),
            AlphaBalance::from(alice_root_stake)
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_on_subnet(&bob, NetUid::ROOT),
            AlphaBalance::from(bob_root_stake)
        );

        // Lets change the take value. (Bob is greedy.)
        ChildkeyTake::<Test>::insert(bob, alpha, PerU16::from_parts(u16::MAX));

        // Lets drain
        let pending_alpha = AlphaBalance::from(1_000_000_000);
        let pending_root2 = TaoBalance::from(1_000_000_000);
        SubtensorModule::distribute_emission(
            alpha,
            pending_alpha.saturating_div(2.into()).into(),
            pending_alpha.saturating_div(2.into()).into(),
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
        );

        // Alice makes nothing
        assert_eq!(
            AlphaDividendsPerSubnet::<Test>::get(alpha, alice),
            AlphaBalance::ZERO
        );
        // Bob makes it all.
        assert_abs_diff_eq!(
            AlphaDividendsPerSubnet::<Test>::get(alpha, bob),
            pending_alpha,
            epsilon = 1.into()
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::root_children_drain::test_get_root_children_drain_half_proportion --exact --show-output --nocapture
#[test]
fn test_get_root_children_drain_half_proportion() {
    new_test_ext(1).execute_with(|| {
        // Init netuid 1
        let alpha = NetUid::from(1);
        add_network(NetUid::ROOT, 1, 0);
        add_network(alpha, 1, 0);
        SubtensorModule::set_ck_burn(0);
        // Set TAO weight to 1.
        SubtensorModule::set_tao_weight(u64::MAX); // Set TAO weight to 1.
        // Create keys.
        let cold_alice = U256::from(0);
        let cold_bob = U256::from(1);
        let alice = U256::from(2);
        let bob = U256::from(3);
        // Register Alice and Bob to the root network and alpha subnet.
        register_ok_neuron(alpha, alice, cold_alice, 0);
        register_ok_neuron(alpha, bob, cold_bob, 0);
        assert_ok!(SubtensorModule::root_register(
            RuntimeOrigin::signed(cold_alice).clone(),
            alice,
        ));
        assert_ok!(SubtensorModule::root_register(
            RuntimeOrigin::signed(cold_bob).clone(),
            bob,
        ));
        // Add stake for Alice and Bob on root.
        let alice_root_stake = AlphaBalance::from(1_000_000_000);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &alice,
            &cold_alice,
            NetUid::ROOT,
            alice_root_stake,
        );
        let bob_root_stake = AlphaBalance::from(1_000_000_000);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &bob,
            &cold_bob,
            NetUid::ROOT,
            alice_root_stake,
        );
        // Add stake for Alice and Bob on netuid.
        let alice_alpha_stake = AlphaBalance::from(1_000_000_000);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &alice,
            &cold_alice,
            alpha,
            alice_alpha_stake,
        );
        let bob_alpha_stake = AlphaBalance::from(1_000_000_000);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &bob,
            &cold_bob,
            alpha,
            bob_alpha_stake,
        );
        // Set Bob as 100% child of Alice on root.
        mock_set_children_no_epochs(alpha, &alice, &[(u64::MAX / 2, bob)]);

        // Set Bob childkey take to zero.
        ChildkeyTake::<Test>::insert(bob, alpha, PerU16::zero());
        Delegates::<Test>::insert(alice, PerU16::zero());
        Delegates::<Test>::insert(bob, PerU16::zero());

        // Lets drain!
        let pending_alpha = AlphaBalance::from(1_000_000_000);
        SubtensorModule::distribute_emission(
            alpha,
            pending_alpha.saturating_div(2.into()).into(),
            pending_alpha.saturating_div(2.into()).into(),
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
        );

        // Alice and Bob make the same amount.
        close(
            AlphaDividendsPerSubnet::<Test>::get(alpha, alice).into(),
            (pending_alpha / 2.into()).into(),
            10,
        );
        close(
            AlphaDividendsPerSubnet::<Test>::get(alpha, bob).into(),
            (pending_alpha / 2.into()).into(),
            10,
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::root_children_drain::test_get_root_children_drain_with_take --exact --show-output --nocapture
#[test]
fn test_get_root_children_drain_with_take() {
    new_test_ext(1).execute_with(|| {
        // Init netuid 1
        let alpha = NetUid::from(1);
        add_network(NetUid::ROOT, 1, 0);
        add_network(alpha, 1, 0);
        // Set TAO weight to 1.
        SubtensorModule::set_tao_weight(u64::MAX); // Set TAO weight to 1.
        // Create keys.
        let cold_alice = U256::from(0);
        let cold_bob = U256::from(1);
        let alice = U256::from(2);
        let bob = U256::from(3);
        // Register Alice and Bob to the root network and alpha subnet.
        register_ok_neuron(alpha, alice, cold_alice, 0);
        register_ok_neuron(alpha, bob, cold_bob, 0);
        assert_ok!(SubtensorModule::root_register(
            RuntimeOrigin::signed(cold_alice).clone(),
            alice,
        ));
        assert_ok!(SubtensorModule::root_register(
            RuntimeOrigin::signed(cold_bob).clone(),
            bob,
        ));
        // Add stake for Alice and Bob on root.
        let alice_root_stake = AlphaBalance::from(1_000_000_000);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &alice,
            &cold_alice,
            NetUid::ROOT,
            alice_root_stake,
        );
        let bob_root_stake = AlphaBalance::from(1_000_000_000);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &bob,
            &cold_bob,
            NetUid::ROOT,
            alice_root_stake,
        );
        // Add stake for Alice and Bob on netuid.
        let alice_alpha_stake = AlphaBalance::from(1_000_000_000);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &alice,
            &cold_alice,
            alpha,
            alice_alpha_stake,
        );
        let bob_alpha_stake = AlphaBalance::from(1_000_000_000);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &bob,
            &cold_bob,
            alpha,
            bob_alpha_stake,
        );
        // Set Bob as 100% child of Alice on root.
        ChildkeyTake::<Test>::insert(bob, alpha, PerU16::from_parts(u16::MAX));
        mock_set_children_no_epochs(alpha, &alice, &[(u64::MAX, bob)]);
        // Set Bob validator take to zero.
        Delegates::<Test>::insert(alice, PerU16::zero());
        Delegates::<Test>::insert(bob, PerU16::zero());

        // Lets drain!
        let pending_alpha = AlphaBalance::from(1_000_000_000);
        SubtensorModule::distribute_emission(
            alpha,
            pending_alpha.saturating_div(2.into()).into(),
            pending_alpha.saturating_div(2.into()).into(),
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
        );

        // Bob makes it all.
        close(
            AlphaDividendsPerSubnet::<Test>::get(alpha, alice).into(),
            0,
            10,
        );
        close(
            AlphaDividendsPerSubnet::<Test>::get(alpha, bob).into(),
            pending_alpha.into(),
            10,
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::root_children_drain::test_get_root_children_drain_with_half_take --exact --show-output --nocapture
#[test]
fn test_get_root_children_drain_with_half_take() {
    new_test_ext(1).execute_with(|| {
        // Init netuid 1
        let alpha = NetUid::from(1);
        add_network(NetUid::ROOT, 1, 0);
        add_network(alpha, 1, 0);
        // Set TAO weight to 1.
        SubtensorModule::set_tao_weight(u64::MAX); // Set TAO weight to 1.
        SubtensorModule::set_ck_burn(0);
        // Create keys.
        let cold_alice = U256::from(0);
        let cold_bob = U256::from(1);
        let alice = U256::from(2);
        let bob = U256::from(3);
        // Register Alice and Bob to the root network and alpha subnet.
        register_ok_neuron(alpha, alice, cold_alice, 0);
        register_ok_neuron(alpha, bob, cold_bob, 0);
        assert_ok!(SubtensorModule::root_register(
            RuntimeOrigin::signed(cold_alice).clone(),
            alice,
        ));
        assert_ok!(SubtensorModule::root_register(
            RuntimeOrigin::signed(cold_bob).clone(),
            bob,
        ));
        // Add stake for Alice and Bob on root.
        let alice_root_stake = AlphaBalance::from(1_000_000_000);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &alice,
            &cold_alice,
            NetUid::ROOT,
            alice_root_stake,
        );
        let bob_root_stake = AlphaBalance::from(1_000_000_000);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &bob,
            &cold_bob,
            NetUid::ROOT,
            alice_root_stake,
        );
        // Add stake for Alice and Bob on netuid.
        let alice_alpha_stake = AlphaBalance::from(1_000_000_000);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &alice,
            &cold_alice,
            alpha,
            alice_alpha_stake,
        );
        let bob_alpha_stake = AlphaBalance::from(1_000_000_000);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &bob,
            &cold_bob,
            alpha,
            bob_alpha_stake,
        );
        // Set Bob as 100% child of Alice on root.
        ChildkeyTake::<Test>::insert(bob, alpha, PerU16::from_parts(u16::MAX / 2));
        mock_set_children_no_epochs(alpha, &alice, &[(u64::MAX, bob)]);
        // Set Bob childkey take to zero.
        Delegates::<Test>::insert(alice, PerU16::zero());
        Delegates::<Test>::insert(bob, PerU16::zero());

        // Lets drain!
        let pending_alpha = AlphaBalance::from(1_000_000_000);
        SubtensorModule::distribute_emission(
            alpha,
            pending_alpha.saturating_div(2.into()).into(),
            pending_alpha.saturating_div(2.into()).into(),
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
        );

        // Alice and Bob make the same amount.
        close(
            AlphaDividendsPerSubnet::<Test>::get(alpha, alice).into(),
            (pending_alpha / 4.into()).into(),
            10000,
        );
        close(
            AlphaDividendsPerSubnet::<Test>::get(alpha, bob).into(),
            3 * u64::from(pending_alpha / 4.into()),
            10000,
        );
    });
}
