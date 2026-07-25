//! Helper tests: `distribute_alpha_pro_rata`.

use super::*;

// ─────────────────────────────────────────────────────────────────────────────
// distribute_alpha_pro_rata
// ─────────────────────────────────────────────────────────────────────────────
//
// Scenario A – buy-dominant, pool rate = 1:1
// ───────────────────────────────────────────
// Both buyers and sellers are present, but buys exceed sells in TAO terms.
// Sellers are settled first (they receive TAO in distribute_tao_pro_rata).
// Their alpha (200 total) stays in the pallet account as passthrough for buyers.
// The residual buy TAO hits the pool and returns 800 alpha (at 1:1 rate).
//
// 3 buyers: Alice 300 TAO net, Bob 200 TAO net, Charlie 500 TAO net (total 1000)
// Sellers contributed 200 alpha (passthrough, no pool interaction).
// Net residual TAO to pool = 1000 - 200 = 800 TAO → pool returns 800 alpha (1:1).
// Total alpha available to buyers = 800 (pool) + 200 (seller passthrough) = 1000.
//
// Pro-rata shares (proportional to each buyer's net TAO):
//   Alice:   1000 * 300 / 1000 = 300 alpha
//   Bob:     1000 * 200 / 1000 = 200 alpha
//   Charlie: 1000 * 500 / 1000 = 500 alpha
//
// Scenario B – sell-dominant
// ───────────────────────────
// Both buyers and sellers are present, but sells exceed buys in TAO terms.
// Buyers are settled from the sellers' alpha directly (no pool for them).
// The residual sell alpha hits the pool; sellers receive TAO in distribute_tao_pro_rata.
//
// 2 buyers: Alice 400 TAO net, Bob 600 TAO net (total 1000)
// Price = 2.0 TAO/alpha → total alpha for buyers = 1000 / 2 = 500 alpha.
//
// Pro-rata shares:
//   Alice:  500 * 400 / 1000 = 200 alpha
//   Bob:    500 * 600 / 1000 = 300 alpha
//
// Scenario C – buy-dominant, pool rate != 1:1
// ────────────────────────────────────────────────────────
// Same structure as Scenario A but the pool returns fewer alpha than the TAO
// sent in, simulating realistic AMM. Pro-rata is computed over
// whatever the pool actually returned — the distribution logic is rate-agnostic.
//
// 3 buyers: Alice 300 TAO net, Bob 200 TAO net, Charlie 500 TAO net (total 1000)
// Sellers contributed 200 alpha (passthrough).
// Net residual TAO to pool = 800 TAO → pool returns 750 alpha (slippage).
// Total alpha available to buyers = 750 (pool) + 200 (seller passthrough) = 950.
//
// Pro-rata shares:
//   Alice:   950 * 300 / 1000 = 285 alpha
//   Bob:     950 * 200 / 1000 = 190 alpha
//   Charlie: 950 * 500 / 1000 = 475 alpha
//
// Scenario D – buy-dominant, indivisible remainder (dust)
// ─────────────────────────────────────────────────────────
// Integer division floors every share. The sum of floors is strictly less than
// total_alpha when total_alpha is not divisible by total_buy_net.
// The leftover alpha stays in the pallet intermediary account (never transferred).
//
// 3 buyers: Alice 1 TAO net, Bob 1 TAO net, Charlie 1 TAO net (total 3)
// Pool returns 10 alpha; no sellers → total_alpha = 10.
//
// Pro-rata shares (floor):
//   Alice:   floor(10 * 1 / 3) = 3 alpha
//   Bob:     floor(10 * 1 / 3) = 3 alpha
//   Charlie: floor(10 * 1 / 3) = 3 alpha
//   Total distributed: 9 alpha
//   Dust remaining in pallet account: 10 - 9 = 1 alpha (never transferred)

#[test]
fn distribute_alpha_pro_rata_buy_dominant_scenario_a() {
    new_test_ext().execute_with(|| {
        // Pool returned 800 alpha; sell-side passthrough = 200 alpha.
        // Total = 1000 alpha distributed across 3 buyers (300, 200, 500 TAO net).
        // Expected shares: Alice 300, Bob 200, Charlie 500.

        let hotkey = AccountKeyring::Dave.to_account_id();
        let entries = bounded_buy_entries(vec![
            make_buy_entry(
                H256::repeat_byte(1),
                alice(),
                hotkey.clone(),
                300,
                300,
                Perbill::zero(),
                fee_recipient(),
            ),
            make_buy_entry(
                H256::repeat_byte(2),
                bob(),
                hotkey.clone(),
                200,
                200,
                Perbill::zero(),
                fee_recipient(),
            ),
            make_buy_entry(
                H256::repeat_byte(3),
                charlie(),
                hotkey.clone(),
                500,
                500,
                Perbill::zero(),
                fee_recipient(),
            ),
        ]);
        let pallet_acct = PalletHotkeyAccount::get(); // reuse as coldkey for brevity
        let pallet_hk = PalletHotkeyAccount::get();

        LimitOrders::<Test>::distribute_alpha_pro_rata(
            &entries,
            800u128,   // actual_out from pool (alpha)
            1_000u128, // total_buy_net (TAO)
            200u128,   // total_sell_net (alpha passthrough)
            &OrderSide::Buy,
            U64F64::from_num(1u32),
            &pallet_acct,
            &pallet_hk,
            netuid(),
        )
        .unwrap();

        let transfers = MockSwap::alpha_transfers();
        // 3 transfers expected (one per buyer)
        assert_eq!(transfers.len(), 3);

        // Check each recipient's amount (signer is to_coldkey).
        let alice_amt = transfers
            .iter()
            .find(|(_, _, to_ck, _, _, _)| to_ck == &alice())
            .unwrap()
            .5;
        let bob_amt = transfers
            .iter()
            .find(|(_, _, to_ck, _, _, _)| to_ck == &bob())
            .unwrap()
            .5;
        let charlie_amt = transfers
            .iter()
            .find(|(_, _, to_ck, _, _, _)| to_ck == &charlie())
            .unwrap()
            .5;

        assert_eq!(alice_amt, 300u64, "Alice should receive 300 alpha");
        assert_eq!(bob_amt, 200u64, "Bob should receive 200 alpha");
        assert_eq!(charlie_amt, 500u64, "Charlie should receive 500 alpha");
    });
}

#[test]
fn distribute_alpha_pro_rata_sell_dominant_scenario_b() {
    new_test_ext().execute_with(|| {
        // Price = 2.0 TAO/alpha; buyers have 400 + 600 = 1000 TAO net.
        // Total alpha = 1000 / 2 = 500.
        // Expected: Alice 200 alpha, Bob 300 alpha.

        let hotkey = AccountKeyring::Dave.to_account_id();
        let entries = bounded_buy_entries(vec![
            make_buy_entry(
                H256::repeat_byte(4),
                alice(),
                hotkey.clone(),
                400,
                400,
                Perbill::zero(),
                fee_recipient(),
            ),
            make_buy_entry(
                H256::repeat_byte(5),
                bob(),
                hotkey.clone(),
                600,
                600,
                Perbill::zero(),
                fee_recipient(),
            ),
        ]);
        let pallet_acct = PalletHotkeyAccount::get();
        let pallet_hk = PalletHotkeyAccount::get();

        LimitOrders::<Test>::distribute_alpha_pro_rata(
            &entries,
            0u128,     // actual_out unused in sell-dominant branch
            1_000u128, // total_buy_net (TAO)
            999u128,   // total_sell_net — doesn't matter for sell-dominant logic
            &OrderSide::Sell,
            U64F64::from_num(2u32), // price = 2 TAO/alpha
            &pallet_acct,
            &pallet_hk,
            netuid(),
        )
        .unwrap();

        let transfers = MockSwap::alpha_transfers();
        assert_eq!(transfers.len(), 2);

        let alice_amt = transfers
            .iter()
            .find(|(_, _, to_ck, _, _, _)| to_ck == &alice())
            .unwrap()
            .5;
        let bob_amt = transfers
            .iter()
            .find(|(_, _, to_ck, _, _, _)| to_ck == &bob())
            .unwrap()
            .5;

        assert_eq!(alice_amt, 200u64, "Alice should receive 200 alpha");
        assert_eq!(bob_amt, 300u64, "Bob should receive 300 alpha");
    });
}

#[test]
fn distribute_alpha_pro_rata_buy_dominant_scenario_c() {
    new_test_ext().execute_with(|| {
        // Scenario C: same buyer setup as A but pool returns 750 alpha (slippage)
        // instead of 800. Proves pro-rata is computed over actual pool output and
        // is therefore rate-agnostic — the distribution logic doesn't assume 1:1.
        //
        // Net residual TAO to pool = 800 TAO → pool returns 750 alpha (not 800).
        // Total alpha = 750 (pool) + 200 (seller passthrough) = 950.
        //
        // Expected shares:
        //   Alice:   950 * 300 / 1000 = 285 alpha
        //   Bob:     950 * 200 / 1000 = 190 alpha
        //   Charlie: 950 * 500 / 1000 = 475 alpha

        let hotkey = AccountKeyring::Dave.to_account_id();
        let entries = bounded_buy_entries(vec![
            make_buy_entry(
                H256::repeat_byte(6),
                alice(),
                hotkey.clone(),
                300,
                300,
                Perbill::zero(),
                fee_recipient(),
            ),
            make_buy_entry(
                H256::repeat_byte(7),
                bob(),
                hotkey.clone(),
                200,
                200,
                Perbill::zero(),
                fee_recipient(),
            ),
            make_buy_entry(
                H256::repeat_byte(8),
                charlie(),
                hotkey.clone(),
                500,
                500,
                Perbill::zero(),
                fee_recipient(),
            ),
        ]);
        let pallet_acct = PalletHotkeyAccount::get();
        let pallet_hk = PalletHotkeyAccount::get();

        LimitOrders::<Test>::distribute_alpha_pro_rata(
            &entries,
            750u128,   // actual_out from pool (750, not 800 — slippage)
            1_000u128, // total_buy_net (TAO)
            200u128,   // total_sell_net (alpha passthrough)
            &OrderSide::Buy,
            U64F64::from_num(1u32),
            &pallet_acct,
            &pallet_hk,
            netuid(),
        )
        .unwrap();

        let transfers = MockSwap::alpha_transfers();
        assert_eq!(transfers.len(), 3);

        let alice_amt = transfers
            .iter()
            .find(|(_, _, to_ck, _, _, _)| to_ck == &alice())
            .unwrap()
            .5;
        let bob_amt = transfers
            .iter()
            .find(|(_, _, to_ck, _, _, _)| to_ck == &bob())
            .unwrap()
            .5;
        let charlie_amt = transfers
            .iter()
            .find(|(_, _, to_ck, _, _, _)| to_ck == &charlie())
            .unwrap()
            .5;

        assert_eq!(
            alice_amt, 285u64,
            "Alice receives 950 * 300/1000 = 285 alpha"
        );
        assert_eq!(bob_amt, 190u64, "Bob receives 950 * 200/1000 = 190 alpha");
        assert_eq!(
            charlie_amt, 475u64,
            "Charlie receives 950 * 500/1000 = 475 alpha"
        );
    });
}

#[test]
fn distribute_alpha_pro_rata_dust_remains_in_pallet_scenario_d() {
    new_test_ext().execute_with(|| {
        // Scenario D: total_alpha = 10, three equal buyers (total_buy_net = 3).
        // floor(10 * 1/3) = 3 each → 9 distributed → 1 alpha dust stays in pallet.

        let hotkey = AccountKeyring::Dave.to_account_id();
        let pallet_acct = PalletHotkeyAccount::get();
        let pallet_hk = PalletHotkeyAccount::get();

        // Seed the pallet account with the 10 alpha it would hold after collect_assets
        // and the pool swap (actual_out=10, no sellers).
        MockSwap::set_alpha_balance(pallet_acct.clone(), pallet_hk.clone(), netuid(), 10);

        let entries = bounded_buy_entries(vec![
            make_buy_entry(
                H256::repeat_byte(9),
                alice(),
                hotkey.clone(),
                1,
                1,
                Perbill::zero(),
                fee_recipient(),
            ),
            make_buy_entry(
                H256::repeat_byte(10),
                bob(),
                hotkey.clone(),
                1,
                1,
                Perbill::zero(),
                fee_recipient(),
            ),
            make_buy_entry(
                H256::repeat_byte(11),
                charlie(),
                hotkey.clone(),
                1,
                1,
                Perbill::zero(),
                fee_recipient(),
            ),
        ]);

        LimitOrders::<Test>::distribute_alpha_pro_rata(
            &entries,
            10u128, // actual_out from pool
            3u128,  // total_buy_net (TAO) — not divisible into 10 evenly
            0u128,  // total_sell_net — no sellers
            &OrderSide::Buy,
            U64F64::from_num(1u32),
            &pallet_acct,
            &pallet_hk,
            netuid(),
        )
        .unwrap();

        let transfers = MockSwap::alpha_transfers();
        assert_eq!(transfers.len(), 3);

        let alice_amt = transfers
            .iter()
            .find(|(_, _, to_ck, _, _, _)| to_ck == &alice())
            .unwrap()
            .5;
        let bob_amt = transfers
            .iter()
            .find(|(_, _, to_ck, _, _, _)| to_ck == &bob())
            .unwrap()
            .5;
        let charlie_amt = transfers
            .iter()
            .find(|(_, _, to_ck, _, _, _)| to_ck == &charlie())
            .unwrap()
            .5;

        assert_eq!(alice_amt, 3u64, "floor(10 * 1/3) = 3");
        assert_eq!(bob_amt, 3u64, "floor(10 * 1/3) = 3");
        assert_eq!(charlie_amt, 3u64, "floor(10 * 1/3) = 3");

        // The pallet account started with 10 and sent out 9 — 1 alpha dust remains
        // in the pallet account, not burnt, not distributed.
        let pallet_remaining = MockSwap::alpha_balance(&pallet_acct, &pallet_hk, netuid());
        assert_eq!(
            pallet_remaining, 1u64,
            "1 alpha dust stays in pallet account, not burnt"
        );
    });
}
