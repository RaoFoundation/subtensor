//! Tests for the 1000× input-reserve swap size cap.

use super::*;

#[test]
fn test_swap_rejects_input_over_1000x_input_reserve() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        TaoReserve::set_mock_reserve(netuid, TaoBalance::from(1_000));
        AlphaReserve::set_mock_reserve(netuid, AlphaBalance::from(1_000));

        assert_noop!(
            Pallet::<Test>::do_swap(
                netuid,
                GetTaoForAlpha::with_amount(1_000_001),
                get_min_price(),
                true,
                false,
            ),
            Error::<Test>::SwapInputTooLarge
        );
        assert_noop!(
            Pallet::<Test>::do_swap(
                netuid,
                GetAlphaForTao::with_amount(1_000_001),
                get_max_price(),
                true,
                false,
            ),
            Error::<Test>::SwapInputTooLarge
        );
    });
}

#[test]
fn test_sim_swap_rejects_input_over_1000x_input_reserve() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        TaoReserve::set_mock_reserve(netuid, TaoBalance::from(1_000));
        AlphaReserve::set_mock_reserve(netuid, AlphaBalance::from(1_000));

        assert_noop!(
            Pallet::<Test>::sim_swap(netuid, GetTaoForAlpha::with_amount(1_001_000)),
            Error::<Test>::SwapInputTooLarge
        );
        assert_noop!(
            Pallet::<Test>::sim_swap(netuid, GetAlphaForTao::with_amount(1_001_000)),
            Error::<Test>::SwapInputTooLarge
        );
    });
}

#[test]
fn test_swap_allows_input_at_1000x_input_reserve() {
    new_test_ext().execute_with(|| {
        let netuid = NetUid::from(1);
        TaoReserve::set_mock_reserve(netuid, TaoBalance::from(1_000));
        AlphaReserve::set_mock_reserve(netuid, AlphaBalance::from(1_000));

        assert_ok!(Pallet::<Test>::do_swap(
            netuid,
            GetTaoForAlpha::with_amount(1_000_000),
            get_min_price(),
            true,
            true,
        ));
        assert_ok!(Pallet::<Test>::do_swap(
            netuid,
            GetAlphaForTao::with_amount(1_000_000),
            get_max_price(),
            true,
            true,
        ));
    });
}

#[allow(dead_code)]
fn bbox(t: U64F64, a: U64F64, b: U64F64) -> U64F64 {
    if t < a {
        a
    } else if t > b {
        b
    } else {
        t
    }
}

#[allow(dead_code)]
fn print_current_price(netuid: NetUid) {
    let current_price = Pallet::<Test>::current_price(netuid);
    log::trace!("Current price: {current_price:.6}");
}
