//! Helper tests: `collect_fees`.

use super::*;

// ─────────────────────────────────────────────────────────────────────────────
// collect_fees
// ─────────────────────────────────────────────────────────────────────────────
//
// Scenario:
// 2 buy orders with fees 50 and 150 TAO → total_buy_fee = 200 TAO.
// sell_fee_tao passed in = 80 TAO.
// Total fee = 280 TAO forwarded to FeeCollector in one transfer.

#[test]
fn collect_fees_forwards_combined_fees_to_collector() {
    new_test_ext().execute_with(|| {
        let hotkey = AccountKeyring::Dave.to_account_id();
        // Buy entries carry fee in field index 5.
        let buys = bounded_buy_entries(vec![
            make_buy_entry(
                H256::repeat_byte(20),
                alice(),
                hotkey.clone(),
                1_000,
                950,
                Perbill::from_parts(50_000_000), // 5% of 1000 = 50
                fee_recipient(),
            ),
            make_buy_entry(
                H256::repeat_byte(21),
                bob(),
                hotkey.clone(),
                1_500,
                1_350,
                Perbill::from_parts(100_000_000), // 10% of 1500 = 150
                fee_recipient(),
            ),
        ]);
        let pallet_acct = PalletHotkeyAccount::get();

        assert_ok!(LimitOrders::<Test>::collect_fees(
            &buys,
            vec![(fee_recipient(), 80u64)],
            &pallet_acct
        ));

        let tao_transfers = MockSwap::tao_transfers();
        assert_eq!(tao_transfers.len(), 1, "single transfer to fee_recipient");
        let (from, to, amount) = &tao_transfers[0];
        assert_eq!(from, &pallet_acct, "fee comes from pallet account");
        assert_eq!(to, &fee_recipient(), "fee goes to fee_recipient");
        assert_eq!(*amount, 280u64, "total fee = 200 (buy) + 80 (sell)");
    });
}

#[test]
fn collect_fees_no_transfer_when_zero_fees() {
    new_test_ext().execute_with(|| {
        // No buy fees, no sell fee.
        let hotkey = AccountKeyring::Dave.to_account_id();
        let buys = bounded_buy_entries(vec![make_buy_entry(
            H256::repeat_byte(22),
            alice(),
            hotkey,
            1_000,
            1_000,
            Perbill::zero(),
            fee_recipient(),
        )]);
        let pallet_acct = PalletHotkeyAccount::get();

        assert_ok!(LimitOrders::<Test>::collect_fees(
            &buys,
            vec![],
            &pallet_acct
        ));

        let tao_transfers = MockSwap::tao_transfers();
        assert_eq!(tao_transfers.len(), 0, "no transfer when total fee is zero");
    });
}
