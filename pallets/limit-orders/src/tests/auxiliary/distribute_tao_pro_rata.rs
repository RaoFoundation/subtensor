//! Helper tests: `distribute_tao_pro_rata`.

use super::*;

// ─────────────────────────────────────────────────────────────────────────────
// distribute_tao_pro_rata
// ─────────────────────────────────────────────────────────────────────────────
//
// Scenario A – sell-dominant, fee = 0
// ─────────────────────────────────────
// Both buyers and sellers are present, but sells exceed buys in TAO terms.
// Buyers are settled first (they receive alpha in distribute_alpha_pro_rata).
// The residual sell alpha hits the pool; pool returns TAO.
// Buy-side TAO also stays in pallet as passthrough for sellers.
//
// 2 sellers: Alice 400 alpha, Bob 600 alpha (total 1000 alpha)
// Price = 2.0 TAO/alpha → sell_tao_equiv: Alice 800, Bob 1200, total 2000.
// Pool returned 1200 TAO for the residual alpha; buy passthrough = 800 TAO.
// Total TAO available to sellers = 1200 (pool) + 800 (buy passthrough) = 2000.
//
// Pro-rata shares (proportional to each seller's TAO-equiv):
//   Alice:  2000 * 800 / 2000 = 800 TAO
//   Bob:    2000 * 1200 / 2000 = 1200 TAO
//
// Scenario B – sell-dominant, fee = 1% (10_000_000 ppb)
// ────────────────────────────────────────────────────────
// Same structure as Scenario A. Fee is deducted from each seller's gross TAO
// payout; the withheld TAO stays in the pallet account for collect_fees.
//
// Alice gross=800, fee=8 (1% of 800), net=792 TAO
// Bob   gross=1200, fee=12, net=1188 TAO
// Total sell fee returned: 20 TAO
//
// Scenario C – buy-dominant
// ──────────────────────────
// Both buyers and sellers are present, but buys exceed sells in TAO terms.
// Sellers receive their alpha valued at current_price — no pool interaction
// for them. The TAO they receive comes from the buyers' collected TAO directly.
//
// 2 sellers: Alice 300 alpha, Bob 200 alpha (total 500 alpha)
// Price = 2.0 TAO/alpha → sell_tao_equiv: Alice 600, Bob 400, total 1000.
// Buy-dominant branch: total_tao = total_sell_tao_equiv = 1000 TAO.
//
// Shares:
//   Alice:  1000 * 600 / 1000 = 600 TAO
//   Bob:    1000 * 400 / 1000 = 400 TAO
//
// Scenario D – sell-dominant, indivisible remainder (dust)
// ─────────────────────────────────────────────────────────
// Integer division floors every gross share. The leftover TAO stays in the
// pallet intermediary account (never transferred, not burnt).
//
// 3 sellers: Alice 1 alpha, Bob 1 alpha, Charlie 1 alpha (total 3 alpha)
// Price = 1.0 TAO/alpha → sell_tao_equiv = 1 each, total_sell_tao_equiv = 3.
// No buyers; actual_out from pool = 10 TAO, buy passthrough = 0.
// total_tao = 10 + 0 = 10.
//
// Pro-rata shares (floor):
//   Alice:   floor(10 * 1 / 3) = 3 TAO
//   Bob:     floor(10 * 1 / 3) = 3 TAO
//   Charlie: floor(10 * 1 / 3) = 3 TAO
//   Total distributed: 9 TAO
//   Dust remaining in pallet account: 10 - 9 = 1 TAO (never transferred)

#[test]
fn distribute_tao_pro_rata_sell_dominant_no_fee_scenario_a() {
    new_test_ext().execute_with(|| {
        // Price = 2, total_tao = 1200 (pool) + 800 (buy passthrough) = 2000
        // Alice alpha=400 → tao_equiv=800; Bob alpha=600 → tao_equiv=1200.
        // total_sell_tao_equiv = 2000.
        // Shares: Alice 800, Bob 1200.

        let hotkey = AccountKeyring::Dave.to_account_id();
        let entries = bounded_sell_entries(vec![
            make_buy_entry(
                H256::repeat_byte(6),
                alice(),
                hotkey.clone(),
                400,
                400,
                Perbill::zero(),
                fee_recipient(),
            ),
            make_buy_entry(
                H256::repeat_byte(7),
                bob(),
                hotkey.clone(),
                600,
                600,
                Perbill::zero(),
                fee_recipient(),
            ),
        ]);
        let pallet_acct = PalletHotkeyAccount::get();

        let sell_fees = LimitOrders::<Test>::distribute_tao_pro_rata(
            &entries,
            1_200u128, // actual_out (pool TAO)
            800u128,   // total_buy_net (buy passthrough TAO)
            2_000u128, // total_sell_tao_equiv (Alice 800 + Bob 1200)
            &OrderSide::Sell,
            U64F64::from_num(2u32),
            &pallet_acct,
            netuid(),
        )
        .unwrap();

        let transfers = MockSwap::tao_transfers();
        assert_eq!(transfers.len(), 2);
        let alice_tao = transfers
            .iter()
            .find(|(_, to, _)| to == &alice())
            .unwrap()
            .2;
        let bob_tao = transfers.iter().find(|(_, to, _)| to == &bob()).unwrap().2;

        assert_eq!(alice_tao, 800u64, "Alice should receive 800 TAO");
        assert_eq!(bob_tao, 1_200u64, "Bob should receive 1200 TAO");
        assert_eq!(
            sell_fees,
            vec![] as Vec<(AccountId, u64)>,
            "No fees at 0 ppb"
        );
    });
}

#[test]
fn distribute_tao_pro_rata_sell_dominant_with_fee_scenario_b() {
    new_test_ext().execute_with(|| {
        // Same setup as above but fee = 10_000_000 ppb = 1%.
        // Alice gross=800, fee=8, net=792; Bob gross=1200, fee=12, net=1188.
        // Total sell fee = 20.

        let hotkey = AccountKeyring::Dave.to_account_id();
        let entries = bounded_sell_entries(vec![
            make_buy_entry(
                H256::repeat_byte(8),
                alice(),
                hotkey.clone(),
                400,
                400,
                Perbill::from_parts(10_000_000),
                fee_recipient(),
            ),
            make_buy_entry(
                H256::repeat_byte(9),
                bob(),
                hotkey.clone(),
                600,
                600,
                Perbill::from_parts(10_000_000),
                fee_recipient(),
            ),
        ]);
        let pallet_acct = PalletHotkeyAccount::get();

        let sell_fees = LimitOrders::<Test>::distribute_tao_pro_rata(
            &entries,
            1_200u128,
            800u128,
            2_000u128,
            &OrderSide::Sell,
            U64F64::from_num(2u32),
            &pallet_acct,
            netuid(),
        )
        .unwrap();

        let transfers = MockSwap::tao_transfers();
        assert_eq!(transfers.len(), 2);
        let alice_tao = transfers
            .iter()
            .find(|(_, to, _)| to == &alice())
            .unwrap()
            .2;
        let bob_tao = transfers.iter().find(|(_, to, _)| to == &bob()).unwrap().2;

        assert_eq!(alice_tao, 792u64, "Alice net after 1% fee on 800");
        assert_eq!(bob_tao, 1_188u64, "Bob net after 1% fee on 1200");
        assert_eq!(
            sell_fees,
            vec![(fee_recipient(), 20u64)],
            "total sell fee = 8 + 12"
        );
    });
}

#[test]
fn distribute_tao_pro_rata_buy_dominant_scenario_c() {
    new_test_ext().execute_with(|| {
        // Buy-dominant: total_tao = total_sell_tao_equiv = 1000.
        // Alice alpha=300 → tao_equiv=600; Bob alpha=200 → tao_equiv=400.
        // Shares: Alice 600, Bob 400.

        let hotkey = AccountKeyring::Dave.to_account_id();
        let entries = bounded_sell_entries(vec![
            make_buy_entry(
                H256::repeat_byte(10),
                alice(),
                hotkey.clone(),
                300,
                300,
                Perbill::zero(),
                fee_recipient(),
            ),
            make_buy_entry(
                H256::repeat_byte(11),
                bob(),
                hotkey.clone(),
                200,
                200,
                Perbill::zero(),
                fee_recipient(),
            ),
        ]);
        let pallet_acct = PalletHotkeyAccount::get();

        let sell_fees = LimitOrders::<Test>::distribute_tao_pro_rata(
            &entries,
            0u128,     // actual_out unused in Buy-dominant branch
            0u128,     // total_buy_net unused in Buy-dominant branch
            1_000u128, // total_sell_tao_equiv (total_tao = this in Buy branch)
            &OrderSide::Buy,
            U64F64::from_num(2u32),
            &pallet_acct,
            netuid(),
        )
        .unwrap();

        let transfers = MockSwap::tao_transfers();
        assert_eq!(transfers.len(), 2);
        let alice_tao = transfers
            .iter()
            .find(|(_, to, _)| to == &alice())
            .unwrap()
            .2;
        let bob_tao = transfers.iter().find(|(_, to, _)| to == &bob()).unwrap().2;

        assert_eq!(alice_tao, 600u64, "Alice should receive 600 TAO");
        assert_eq!(bob_tao, 400u64, "Bob should receive 400 TAO");
        assert_eq!(sell_fees, vec![] as Vec<(AccountId, u64)>);
    });
}

#[test]
fn distribute_tao_pro_rata_dust_remains_in_pallet_scenario_d() {
    new_test_ext().execute_with(|| {
        // Scenario D: total_tao = 10, three equal sellers (total_sell_tao_equiv = 3).
        // floor(10 * 1/3) = 3 each → 9 distributed → 1 TAO dust stays in pallet.

        let hotkey = AccountKeyring::Dave.to_account_id();
        let pallet_acct = PalletHotkeyAccount::get();

        // Seed the pallet account with the 10 TAO it would hold after collect_assets
        // and the pool swap (actual_out=10, no buyers).
        MockSwap::set_tao_balance(pallet_acct.clone(), 10);

        let entries = bounded_sell_entries(vec![
            make_buy_entry(
                H256::repeat_byte(12),
                alice(),
                hotkey.clone(),
                1,
                1,
                Perbill::zero(),
                fee_recipient(),
            ),
            make_buy_entry(
                H256::repeat_byte(13),
                bob(),
                hotkey.clone(),
                1,
                1,
                Perbill::zero(),
                fee_recipient(),
            ),
            make_buy_entry(
                H256::repeat_byte(14),
                charlie(),
                hotkey.clone(),
                1,
                1,
                Perbill::zero(),
                fee_recipient(),
            ),
        ]);

        let sell_fees = LimitOrders::<Test>::distribute_tao_pro_rata(
            &entries,
            10u128, // actual_out from pool (TAO)
            0u128,  // total_buy_net — no buyers
            3u128,  // total_sell_tao_equiv — not divisible into 10 evenly
            &OrderSide::Sell,
            U64F64::from_num(1u32),
            &pallet_acct,
            netuid(),
        )
        .unwrap();

        let transfers = MockSwap::tao_transfers();
        assert_eq!(transfers.len(), 3);

        let alice_tao = transfers
            .iter()
            .find(|(_, to, _)| to == &alice())
            .unwrap()
            .2;
        let bob_tao = transfers.iter().find(|(_, to, _)| to == &bob()).unwrap().2;
        let charlie_tao = transfers
            .iter()
            .find(|(_, to, _)| to == &charlie())
            .unwrap()
            .2;

        assert_eq!(alice_tao, 3u64, "floor(10 * 1/3) = 3");
        assert_eq!(bob_tao, 3u64, "floor(10 * 1/3) = 3");
        assert_eq!(charlie_tao, 3u64, "floor(10 * 1/3) = 3");
        assert_eq!(sell_fees, vec![] as Vec<(AccountId, u64)>);

        // The pallet account started with 10 TAO and sent out 9 — 1 TAO dust remains,
        // not burnt, not distributed.
        let pallet_remaining = MockSwap::tao_balance(&pallet_acct);
        assert_eq!(
            pallet_remaining, 1u64,
            "1 TAO dust stays in pallet account, not burnt"
        );
    });
}
